import { chromium, expect } from '@playwright/test'
import assert from 'node:assert/strict'
import { spawn, execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { mkdir, mkdtemp, readFile, writeFile, rmdir, readdir } from 'node:fs/promises'
import { createServer, createConnection } from 'node:net'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createInterface } from 'node:readline'
import { createReadStream } from 'node:fs'
import { createHash } from 'node:crypto'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
if (process.platform !== 'win32') throw new Error('Native smoke requires Windows and WebView2')
const executable = process.env.SSH_TUNNEL_TEST_BINARY ?? path.join(root, process.argv.includes('--release') ? 'target/release/ssh-tunnel.exe' : 'target/debug/ssh-tunnel.exe')
if (!path.isAbsolute(executable)) throw new Error('SSH_TUNNEL_TEST_BINARY must be an absolute path')
const outputRoot = path.join(root, 'target', 'native-smoke')
await mkdir(outputRoot, { recursive: true })
const fixture = await mkdtemp(path.join(outputRoot, 'run-'))
const implicitConfig = process.argv.includes('--implicit-config')
if (implicitConfig && !process.argv.includes('--release')) throw new Error('Implicit cutover acceptance requires a release build')
const fixtureHome = path.join(fixture, 'home')
const configDirectory = implicitConfig ? path.join(fixtureHome, '.ssh-tunnel') : fixture
await mkdir(configDirectory, { recursive: true })
// WebView2 also needs coherent AppData/temp paths when the test changes home.
const syntheticEnvironment = implicitConfig ? {
  USERPROFILE: fixtureHome, HOME: fixtureHome,
  LOCALAPPDATA: path.join(fixtureHome, 'AppData', 'Local'),
  APPDATA: path.join(fixtureHome, 'AppData', 'Roaming'),
  TEMP: path.join(fixtureHome, 'AppData', 'Local', 'Temp'),
  TMP: path.join(fixtureHome, 'AppData', 'Local', 'Temp'),
} : {}
for (const directory of Object.values(syntheticEnvironment)) await mkdir(directory, { recursive: true })
const legacyOriginal = '# preserved configuration fixture\nversion = 1\nauto_run = false\ntraffic_monitor_enabled = true\n'
const executableHash = createHash('sha256')
for await (const chunk of createReadStream(executable)) executableHash.update(chunk)
const binarySha256 = executableHash.digest('hex')
await writeFile(path.join(fixture, 'binary.json'), JSON.stringify({ executable, sha256: binarySha256, implicitConfig, sandboxDisabledForTest: process.env.SSH_TUNNEL_TEST_NO_SANDBOX === '1' }, null, 2))
const execFileAsync = promisify(execFile)
const autostartName = `ssh-tunnel-test-${path.basename(fixture)}`
const runKey = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run'
const config = path.join(configDirectory, 'config.toml')
if (implicitConfig) await writeFile(config, legacyOriginal)
const socket = createServer()
await new Promise(resolve => socket.listen(0, '127.0.0.1', resolve))
const port = socket.address().port
await new Promise(resolve => socket.close(resolve))
const controlledEnvironmentKeys = new Set(['SSH_TUNNEL_CONFIG', 'SSH_TUNNEL_STARTUP_TRACE', 'SSH_TUNNEL_AUTOSTART_NAME', 'SSH_TUNNEL_KNOWN_HOSTS', 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', 'WEBVIEW2_USER_DATA_FOLDER'])
// Node passes only one key for case-insensitive duplicates on Windows.
const inheritedEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !controlledEnvironmentKeys.has(key.toUpperCase()) && !(key.toUpperCase() in syntheticEnvironment)))
// Hosted Windows service sessions can require this test-only WebView2 flag.
// The shipped application never sets it; local default-sandbox coverage is separate.
const sandboxArgument = process.env.SSH_TUNNEL_TEST_NO_SANDBOX === '1' ? ' --no-sandbox' : ''
function launchApp(debugPort) { return spawn(executable, implicitConfig ? [] : ['--config', config], {
  cwd: root,
  windowsHide: true,
  env: { ...inheritedEnv,
    ...syntheticEnvironment,
    SSH_TUNNEL_STARTUP_TRACE: path.join(fixture, `native-startup-${debugPort}.log`),
    SSH_TUNNEL_AUTOSTART_NAME: autostartName,
    SSH_TUNNEL_KNOWN_HOSTS: path.join(fixture, 'known_hosts'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort}${sandboxArgument}`,
    WEBVIEW2_USER_DATA_FOLDER: path.join(fixture, 'webview') },
  stdio: ['ignore', 'pipe', 'pipe'],
}) }
let child = launchApp(port)
let output = ''
child.stdout.on('data', chunk => { output += chunk })
child.stderr.on('data', chunk => { output += chunk })
let launchError
child.on('error', error => { launchError = error })
let browser
let sshFixture
let echoServer
let lastStartupTransport = null
const echoConnections = new Set()
try {
  const endpoint = `http://127.0.0.1:${port}`
  const deadline = Date.now() + 45_000
  while (true) {
    if (launchError) throw launchError
    if (child.exitCode !== null) throw new Error(`Native app exited: ${output}`)
    try {
      const response = await fetch(`${endpoint}/json/version`, { signal: AbortSignal.timeout(2000) })
      lastStartupTransport = { status: response.status }
      if (response.ok) break
    } catch (error) { lastStartupTransport = { error: String(error), cause: error.cause?.code ?? null } }
    if (Date.now() >= deadline) throw new Error(`Native WebView2 startup timeout: ${output}`)
    await new Promise(resolve => setTimeout(resolve, 250))
  }
  browser = await chromium.connectOverCDP(endpoint)
  const context = browser.contexts()[0]
  let page = context.pages()[0]
  if (!page) page = await context.waitForEvent('page')
  await page.waitForFunction(() => window.__TAURI_INTERNALS__?.invoke)
  const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args })
  assert.equal((await invoke('get_state')).tunnels.length, 0)
  assert.equal(await invoke('get_config_path'), config)
  if (implicitConfig) assert.equal(await readFile(config, 'utf8'), legacyOriginal, 'Startup must not rewrite the legacy config')
  sshFixture = spawn(path.join(root, 'target/debug/examples/host_key_fixture.exe'), [], { cwd: root, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] })
  const lines = createInterface({ input: sshFixture.stdout })
  function nextFixtureLine() {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { lines.off('line', receive); reject(new Error('SSH fixture response timeout')) }, 5000)
      function receive(line) { clearTimeout(timer); resolve(JSON.parse(line)) }
      lines.once('line', receive)
    })
  }
  const ssh = await nextFixtureLine()
  echoServer = createServer(connection => { echoConnections.add(connection); connection.on('close', () => echoConnections.delete(connection)); connection.pipe(connection) })
  await new Promise(resolve => echoServer.listen(0, '127.0.0.1', resolve))
  const echoPort = echoServer.address().port
  const reservation = createServer()
  await new Promise(resolve => reservation.listen(0, '127.0.0.1', resolve))
  const localPort = reservation.address().port
  await new Promise(resolve => reservation.close(resolve))
  const navigation = page.getByRole('navigation', { name: 'Main navigation' })
  const sshConfigPath = path.join(fixture, 'ssh-config')
  await writeFile(sshConfigPath, 'Include aliases.conf\n')
  await writeFile(path.join(fixture, 'aliases.conf'), `Host imported-alias\n HostName 127.0.0.1\n User fixture\n Port ${ssh.port}\n`)
  await navigation.getByRole('button', { name: 'Jumpers', exact: true }).click()
  await page.getByRole('button', { name: 'Import SSH config', exact: true }).click()
  await page.getByLabel('SSH config path', { exact: true }).fill(sshConfigPath)
  await page.getByRole('button', { name: 'Load aliases', exact: true }).click()
  await page.getByRole('button', { name: 'Review and import', exact: true }).waitFor()
  await page.setViewportSize({ width: 390, height: 760 })
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  await page.screenshot({ path: path.join(fixture, 'ssh-import-390.png') })
  await page.getByRole('button', { name: 'Review and import', exact: true }).click()
  assert.equal(await page.getByLabel('Host', { exact: true }).inputValue(), '127.0.0.1')
  assert.equal(await page.getByLabel('SSH port', { exact: true }).inputValue(), String(ssh.port))
  await page.getByRole('button', { name: 'Save jumper', exact: true }).click()
  await expect(page.getByRole('button', { name: 'Already imported', exact: true })).toBeDisabled()
  if (implicitConfig) {
    const backups = (await readdir(configDirectory)).filter(name => name.startsWith('config-backup-'))
    assert.ok((await Promise.all(backups.map(name => readFile(path.join(configDirectory, name), 'utf8')))).includes(legacyOriginal), 'First mutation must preserve exact legacy bytes')
  }
  await writeFile(sshConfigPath, 'Host bulk-one bulk-two\n Include batch.inc\n')
  await writeFile(path.join(fixture, 'batch.inc'), 'HostName %n.invalid\n User fixture\n Port 2222\n')
  await page.getByRole('button', { name: 'Load aliases', exact: true }).click()
  await page.getByRole('button', { name: 'Select all', exact: true }).click()
  await page.getByRole('button', { name: 'Import selected (2)', exact: true }).click()
  await page.getByRole('dialog', { name: 'Import selected aliases?' }).getByRole('button', { name: 'Import selected', exact: true }).click()
  await page.getByText('Imported: 2; Duplicates skipped: 0', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Close importer', exact: true }).click()
  const importedJumper = (await invoke('get_state')).jumpers[0]
  assert.equal(importedJumper.name, 'imported-alias')
  assert.equal(importedJumper.bypass_host_verification, false)
  const importedJumpers = (await invoke('get_state')).jumpers
  assert.equal(importedJumpers.length, 3)
  assert.equal(importedJumpers[1].host, 'bulk-one.invalid')
  assert.equal(importedJumpers[2].port, 2222)
  for (const jumper of importedJumpers) await invoke('delete_jumper', { id: jumper.id })
  // Fixture-only IPC cleanup bypasses UI actions; reload before testing new selection state.
  await page.reload()
  await page.getByRole('heading', { name: 'All', exact: true }).waitFor()
  await navigation.getByRole('button', { name: 'Tunnels', exact: true }).click()
  await page.getByRole('button', { name: 'Import SSH command', exact: true }).click()
  await page.getByLabel('SSH command', { exact: true }).fill(`ssh -N -L${localPort}:127.0.0.1:${echoPort} -R29999:127.0.0.1:3000 -D29998 -p${ssh.port} fixture@127.0.0.1`)
  await page.getByRole('button', { name: 'Parse command', exact: true }).click()
  await page.getByRole('button', { name: 'Review authentication', exact: true }).click()
  await page.getByRole('dialog', { name: 'New jumper', exact: true }).waitFor()
  await page.screenshot({ path: path.join(fixture, 'command-auth-review-390.png') })
  await page.getByRole('dialog', { name: 'New jumper', exact: true }).getByLabel('Authentication').selectOption('password')
  await page.getByLabel('Password', { exact: true }).fill('fixture-only')
  const draftJumperDialog = page.getByRole('dialog', { name: 'New jumper', exact: true })
  await draftJumperDialog.getByRole('button', { name: 'Test connection', exact: true }).click()
  await page.getByRole('dialog', { name: 'Trust this server?' }).getByRole('button', { name: 'Cancel connection', exact: true }).last().click()
  await page.getByRole('dialog', { name: 'Connection test', exact: true }).getByRole('button', { name: 'Close test', exact: true }).last().click()
  assert.equal((await invoke('get_state')).jumpers.length, 0)
  assert.equal((await invoke('get_state')).trusted_hosts.length, 0)
  const stalledSockets = new Set()
  const stalledServer = createServer(socket => { stalledSockets.add(socket); socket.on('close', () => stalledSockets.delete(socket)); socket.on('error', () => {}); socket.resume() })
  await new Promise(resolve => stalledServer.listen(0, '127.0.0.1', resolve))
  try {
    await draftJumperDialog.getByLabel('SSH port', { exact: true }).fill(String(stalledServer.address().port))
    await draftJumperDialog.getByRole('button', { name: 'Test connection', exact: true }).click()
    await expect.poll(() => stalledSockets.size).toBe(1)
    const cancelledProbe = page.getByRole('dialog', { name: 'Connection test', exact: true })
    await cancelledProbe.getByRole('button', { name: 'Cancel test', exact: true }).click()
    await expect(cancelledProbe.getByText('The test was cancelled.', { exact: true })).toBeVisible()
    await expect.poll(() => stalledSockets.size).toBe(0)
    await cancelledProbe.getByRole('button', { name: 'Close test', exact: true }).last().click()
    await draftJumperDialog.getByLabel('SSH port', { exact: true }).fill(String(ssh.port))
  } finally {
    for (const socket of stalledSockets) socket.destroy()
    await new Promise(resolve => stalledServer.close(resolve))
  }
  await page.getByRole('button', { name: 'Save jumper', exact: true }).click()
  await page.getByRole('dialog', { name: 'New jumper', exact: true }).waitFor({ state: 'hidden' })
  assert.equal((await invoke('get_state')).jumpers.length, 0, 'Command review must not save partial jumpers')
  await page.screenshot({ path: path.join(fixture, 'command-import-390.png') })
  await page.getByRole('button', { name: 'Import selected (3)', exact: true }).click()
  await page.getByRole('dialog', { name: 'Import selected tunnels?' }).getByRole('button', { name: 'Import selected', exact: true }).click()
  await page.getByText('Imported: 3; Duplicates skipped: 0', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Close importer', exact: true }).click()
  const commandState = await invoke('get_state')
  assert.deepEqual(commandState.tunnels.map(tunnel => tunnel.mode), ['local', 'remote', 'dynamic'])
  assert.equal(commandState.tunnels[1].remote_port, 29999)
  assert.equal(commandState.tunnels[1].local_port, 3000)
  assert.ok(commandState.tunnels.every(tunnel => tunnel.status === 'stopped'))
  assert.equal(commandState.jumpers.length, 1)
  assert.equal(commandState.jumpers[0].bypass_host_verification, false)
  for (const tunnel of commandState.tunnels) await invoke('delete_tunnel', { id: tunnel.id })
  await invoke('delete_jumper', { id: commandState.jumpers[0].id })
  await page.getByRole('button', { name: 'New tunnel', exact: true }).click()
  await page.getByLabel('Name', { exact: true }).fill('Native IPC fixture')
  await page.getByRole('button', { name: 'New jumper', exact: true }).click()
  await page.getByRole('dialog', { name: 'New jumper', exact: true }).getByLabel('Name', { exact: true }).fill('Local fixture')
  await page.getByLabel('Host', { exact: true }).fill('127.0.0.1')
  await page.getByLabel('SSH port', { exact: true }).fill(String(ssh.port))
  await page.getByLabel('User', { exact: true }).fill('fixture')
  await page.getByLabel('Authentication').selectOption('password')
  await page.getByLabel('Password', { exact: true }).fill('fixture-only')
  await page.getByRole('button', { name: 'Save jumper' }).click()
  await page.getByRole('dialog', { name: 'New jumper', exact: true }).waitFor({ state: 'hidden' })
  const jumper = (await invoke('get_state')).jumpers[0].id
  assert.equal(await page.getByLabel('Name', { exact: true }).inputValue(), 'Native IPC fixture')
  assert.equal(await page.getByLabel('Local host').inputValue(), '127.0.0.1')
  assert.equal(await page.getByLabel('Remote host').inputValue(), '127.0.0.1')
  await page.getByLabel('Name', { exact: true }).fill('Native IPC fixture')
  await page.getByLabel('Local port', { exact: true }).fill(String(localPort))
  await page.getByLabel('Remote port', { exact: true }).fill(String(echoPort))
  assert.equal(await page.locator('.chain-list li').count(), 1)
  await page.mouse.click(2, 2)
  assert.equal(await page.getByRole('dialog', { name: 'New tunnel', exact: true }).count(), 1, 'Editing must not close on backdrop')
  await page.getByRole('button', { name: 'Save tunnel' }).click()
  await page.getByRole('dialog').waitFor({ state: 'hidden' })
  const tunnel = (await invoke('get_state')).tunnels[0].id
  const originalJumper = (await invoke('get_state')).jumpers[0]
  await navigation.getByRole('button', { name: 'Jumpers', exact: true }).click()
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  await page.screenshot({ path: path.join(fixture, 'jumpers-390.png') })
  await page.getByRole('button', { name: 'Duplicate jumper: Local fixture', exact: true }).click()
  const copyEditor = page.getByRole('dialog', { name: 'New jumper', exact: true })
  assert.equal(await copyEditor.getByLabel('Name', { exact: true }).inputValue(), 'Local fixture copy')
  assert.equal((await invoke('get_state')).jumpers.length, 1, 'Opening duplicate must not write configuration')
  await copyEditor.getByRole('button', { name: 'Save jumper', exact: true }).click()
  await expect(copyEditor).not.toBeVisible()
  const copiedJumper = (await invoke('get_state')).jumpers.find(item => item.id !== jumper)
  assert.equal(copiedJumper.password, originalJumper.password)
  assert.equal(copiedJumper.host, originalJumper.host)
  assert.equal((await invoke('get_state')).jumpers.find(item => item.id === jumper).name, 'Local fixture')
  await invoke('delete_jumper', { id: copiedJumper.id })
  await page.reload()
  await page.getByRole('heading', { name: 'All', exact: true }).waitFor()
  await page.getByRole('button', { name: 'Manage groups' }).click()
  await page.getByLabel('Group name').fill('Fixture group')
  await page.getByRole('checkbox', { name: 'Native IPC fixture' }).check()
  await page.getByRole('button', { name: 'Save group' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Fixture group', exact: true }).waitFor()
  const group = (await invoke('get_state')).groups[0].id
  await page.getByLabel('Group name').fill('Second group')
  await page.getByRole('checkbox', { name: 'Native IPC fixture' }).check()
  await page.getByRole('button', { name: 'Save group' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Second group', exact: true }).click()
  const secondGroup = (await invoke('get_state')).groups[1].id
  await page.getByRole('button', { name: 'Move up', exact: true }).click()
  await expect.poll(async () => (await invoke('get_state')).groups.map(group => group.id)).toEqual([secondGroup, group])
  await expect(page.getByRole('button', { name: 'Move up', exact: true })).toBeDisabled()
  assert.deepEqual((await invoke('get_state')).groups.map(group => group.tunnel_ids), [[tunnel], [tunnel]])
  await page.getByRole('button', { name: 'Close group manager' }).click()
  await page.reload()
  await page.getByRole('heading', { name: 'All', exact: true }).waitFor()
  const groupNames = await page.getByRole('navigation', { name: 'Tunnel groups' }).getByRole('button').allTextContents()
  assert.ok(groupNames.findIndex(name => name.includes('Second group')) < groupNames.findIndex(name => name.includes('Fixture group')))
  assert.equal(await page.locator('article').count(), 1)
  await page.getByRole('button', { name: /Fixture group/ }).click()
  assert.equal(await page.locator('article').count(), 1)
  for (const width of [1120, 390]) {
    await page.setViewportSize({ width, height: 760 })
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `Horizontal overflow at ${width}px`)
    await page.screenshot({ path: path.join(fixture, `preview-${width}.png`) })
  }
  await page.getByRole('button', { name: 'Details: Native IPC fixture' }).click()
  await page.getByRole('dialog', { name: 'Native IPC fixture' }).waitFor()
  await page.screenshot({ path: path.join(fixture, 'details-390.png') })
  await page.mouse.click(2, 2)
  await page.getByRole('dialog').waitFor({ state: 'hidden' })
  // Poll on the Node side so readiness waits for each native IPC response.
  const waitStatus = status => expect.poll(async () => {
    const state = await invoke('get_state')
    return state.tunnels.find(item => item.id === tunnel)?.status
  }, { timeout: 30_000 }).toBe(status)
  await page.locator('article').getByRole('button', { name: 'Start', exact: true }).click()
  await page.getByRole('dialog', { name: 'Trust this server?' }).waitFor()
  assert.equal(await page.getByText(ssh.fingerprint, { exact: true }).count(), 1)
  const initial = (await invoke('get_pending_trust'))[0]
  await page.getByRole('dialog').getByRole('button', { name: 'Cancel connection', exact: true }).last().click()
  await waitStatus('stopped')
  assert.equal((await invoke('get_state')).trusted_hosts.length, 0)
  await page.locator('article').getByRole('button', { name: 'Start', exact: true }).click()
  await page.getByRole('dialog', { name: 'Trust this server?' }).waitFor()
  await page.screenshot({ path: path.join(fixture, 'first-trust-390.png') })
  await page.getByRole('button', { name: 'Trust key and connect' }).click()
  await waitStatus('running')
  const payload = Buffer.from('native-traffic-fixture')
  await new Promise((resolve, reject) => {
    const connection = createConnection({ host: '127.0.0.1', port: localPort })
    const timer = setTimeout(() => { connection.destroy(); reject(new Error('Forwarded payload timeout')) }, 5000)
    let received = Buffer.alloc(0)
    connection.on('connect', () => connection.write(payload))
    connection.on('error', error => { clearTimeout(timer); reject(error) })
    connection.on('data', chunk => {
      received = Buffer.concat([received, chunk])
      if (received.length >= payload.length) { clearTimeout(timer); assert.deepEqual(received, payload); connection.end(); resolve() }
    })
  })
  await expect.poll(async () => {
    const runtime = await invoke('get_runtime')
    const state = runtime.find(item => item.tunnel_id === tunnel)
    return [state?.traffic.uploaded, state?.traffic.downloaded, state?.traffic_last_hour.uploaded, state?.traffic_last_hour.downloaded]
  }, { timeout: 30_000 }).toEqual([payload.length, payload.length, payload.length, payload.length])
  await page.getByRole('button', { name: 'Details: Native IPC fixture' }).click()
  await page.getByRole('dialog').locator('.traffic-grid small').filter({ hasText: new RegExp(`^Last hour: ${payload.length} B$`) }).first().waitFor()
  await page.screenshot({ path: path.join(fixture, 'traffic-390.png') })
  await page.getByRole('button', { name: 'Close details' }).click()
  const connectedGeneration = (await invoke('get_runtime'))[0].generation
  await navigation.getByRole('button', { name: 'Settings', exact: true }).click()
  await page.getByRole('switch', { name: 'Traffic monitoring', exact: true }).click()
  await expect(page.getByRole('switch', { name: 'Traffic monitoring', exact: true })).not.toBeChecked()
  await expect.poll(async () => (await invoke('get_state')).traffic_monitor_enabled).toBe(false)
  assert.match(await readFile(config, 'utf8'), /traffic_monitor_enabled = false/)
  await page.reload()
  await page.getByRole('button', { name: 'Details: Native IPC fixture' }).click()
  await expect(page.getByRole('dialog').getByText('Traffic monitoring is disabled.', { exact: true })).toBeVisible()
  assert.equal(await page.getByRole('dialog').locator('.traffic-grid').count(), 0)
  assert.equal((await invoke('get_runtime'))[0].generation, connectedGeneration)
  await page.getByRole('button', { name: 'Close details' }).click()
  await navigation.getByRole('button', { name: 'Overview', exact: true }).click()
  await expect(page.getByText('Traffic monitoring is disabled.', { exact: true })).toBeVisible()
  await navigation.getByRole('button', { name: 'Settings', exact: true }).click()
  await page.getByRole('switch', { name: 'Traffic monitoring', exact: true }).click()
  await expect(page.getByRole('switch', { name: 'Traffic monitoring', exact: true })).toBeChecked()
  await navigation.getByRole('button', { name: 'Tunnels', exact: true }).click()
  await page.getByRole('button', { name: 'Details: Native IPC fixture' }).click()
  await page.getByRole('dialog').locator('.traffic-grid small').filter({ hasText: new RegExp(`^Last hour: ${payload.length} B$`) }).first().waitFor()
  await page.getByRole('button', { name: 'Close details' }).click()
  assert.equal((await invoke('get_state')).jumpers[0].bypass_host_verification, false)
  await assert.rejects(invoke('resolve_host_key', { id: tunnel, generation: initial.generation, accept: true }))
  await page.locator('article').getByRole('button', { name: 'Stop', exact: true }).click()
  await waitStatus('stopped')
  const rotatedPromise = nextFixtureLine()
  sshFixture.stdin.write('rotate\n')
  const rotated = await rotatedPromise
  await page.locator('article').getByRole('button', { name: 'Start', exact: true }).click()
  await page.getByRole('dialog', { name: 'Server identity changed' }).waitFor()
  assert.equal(await page.getByText(ssh.fingerprint, { exact: true }).count(), 1)
  assert.equal(await page.getByText(rotated.fingerprint, { exact: true }).count(), 1)
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  await page.screenshot({ path: path.join(fixture, 'changed-key-390.png') })
  await page.getByRole('button', { name: 'Replace saved key and reconnect' }).click()
  await waitStatus('running')
  const trusted = await invoke('get_state')
  assert.equal(trusted.trusted_hosts.length, 1)
  assert.equal(trusted.jumpers[0].bypass_host_verification, false)
  await invoke('stop_tunnel', { id: tunnel })
  const beforeFailure = (await invoke('get_state')).tunnels.find(item => item.id === tunnel)
  await invoke('save_tunnel', { tunnel: { ...beforeFailure, reconnect_mode: 'disabled' } })
  const occupiedPort = createServer()
  await new Promise(resolve => occupiedPort.listen(localPort, '127.0.0.1', resolve))
  try {
    await invoke('start_tunnel', { id: tunnel })
    await waitStatus('error')
    await page.getByRole('button', { name: 'Details: Native IPC fixture' }).click()
    // Test the application copy action without replacing the user's OS clipboard.
    await page.evaluate(() => { Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async text => { window.__smokeCopiedError = text } } }) })
    await page.getByRole('button', { name: 'Copy error', exact: true }).click()
    await expect(page.getByRole('button', { name: 'Copied', exact: true })).toBeVisible()
    assert.equal(await page.evaluate(() => window.__smokeCopiedError), (await invoke('get_state')).tunnels[0].last_error)
    assert.ok((await invoke('get_tunnel_logs', { id: tunnel })).every(entry => entry.timestamp_ms > 0))
    await page.getByRole('button', { name: 'Close details' }).click()
    await navigation.getByRole('button', { name: 'Logs', exact: true }).click()
    await page.getByRole('button', { name: 'Errors', exact: true }).click()
    await expect(page.getByRole('log').locator('.journal-entry').first()).toBeVisible()
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
    await page.screenshot({ path: path.join(fixture, 'global-errors-390.png') })
    await page.getByRole('button', { name: 'Info', exact: true }).click()
    await expect(page.getByRole('log').getByText('Save tunnel: Succeeded', { exact: true }).first()).toBeVisible()
    const journalText = JSON.stringify(await invoke('get_activity_logs'))
    assert.ok(!journalText.includes('fixture-only'), 'Journal must not record authentication payloads')
  } finally {
    await new Promise(resolve => occupiedPort.close(resolve))
    await invoke('stop_tunnel', { id: tunnel })
    await invoke('save_tunnel', { tunnel: beforeFailure })
  }
  const failureToast = page.getByRole('alert').filter({ hasText: 'Local port listener' })
  await expect(failureToast).toContainText('already in use')
  await failureToast.getByRole('button', { name: 'Dismiss error', exact: true }).click()
  await expect(failureToast).not.toBeVisible()
  await navigation.getByRole('button', { name: 'Tunnels', exact: true }).click()
  await page.getByRole('button', { name: 'Details: Native IPC fixture' }).click()
  await page.getByRole('button', { name: 'Diagnose connection', exact: true }).click()
  let probeDialog = page.getByRole('dialog', { name: 'Connection test', exact: true })
  await expect(probeDialog.getByText('Connection test passed', { exact: true })).toBeVisible()
  await expect(probeDialog.getByText('First-hop DNS', { exact: true })).toBeVisible()
  assert.equal((await invoke('get_state')).tunnels[0].status, 'stopped')
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  await page.screenshot({ path: path.join(fixture, 'diagnostics-390.png') })
  await probeDialog.getByRole('button', { name: 'Close test', exact: true }).last().click()
  const probeRotationPromise = nextFixtureLine()
  sshFixture.stdin.write('rotate\n')
  const probeRotation = await probeRotationPromise
  await page.getByRole('button', { name: 'Test connection', exact: true }).click()
  const probeTrust = page.getByRole('dialog', { name: 'Server identity changed' })
  await expect(probeTrust.getByText(probeRotation.fingerprint, { exact: true })).toBeVisible()
  await probeTrust.getByRole('button', { name: 'Replace saved key and reconnect' }).click()
  probeDialog = page.getByRole('dialog', { name: 'Connection test', exact: true })
  await expect(probeDialog.getByText('Connection test passed', { exact: true })).toBeVisible()
  assert.equal((await invoke('get_state')).tunnels[0].status, 'stopped')
  await probeDialog.getByRole('button', { name: 'Close test', exact: true }).last().click()
  await page.getByRole('button', { name: 'Close details' }).click()
  await navigation.getByRole('button', { name: 'Overview', exact: true }).click()
  assert.equal(await page.locator('.summary-grid > div').count(), 3)
  await navigation.getByRole('button', { name: 'Settings', exact: true }).click()
  assert.equal(await page.getByLabel('Language').locator('option').count(), 2)
  const loginToggle = page.getByRole('switch', { name: 'Launch at login', exact: true })
  await expect(loginToggle).not.toBeChecked()
  await loginToggle.click()
  await expect(loginToggle).toBeChecked()
  assert.equal((await invoke('get_state')).auto_run, true)
  const registration = await execFileAsync('reg.exe', ['query', runKey, '/v', autostartName], { windowsHide: true })
  assert.ok(registration.stdout.includes(`"${executable}" --config "${config}"`))
  await loginToggle.click()
  await expect(loginToggle).not.toBeChecked()
  assert.equal((await invoke('get_state')).auto_run, false)
  await assert.rejects(execFileAsync('reg.exe', ['query', runKey, '/v', autostartName], { windowsHide: true }))
  assert.equal(await invoke('get_config_path'), config)
  const exportedPath = path.join(fixture, 'export.toml')
  await page.getByLabel('Configuration file', { exact: true }).fill(exportedPath)
  await page.getByRole('button', { name: 'Export configuration', exact: true }).click()
  await page.getByRole('dialog', { name: 'Confirm export' }).getByRole('button', { name: 'Export configuration', exact: true }).click()
  await page.getByText('Configuration exported.', { exact: true }).waitFor()
  const exported = await readFile(exportedPath, 'utf8')
  assert.match(exported, /Native IPC fixture/)
  await writeFile(exportedPath, exported.replace('Native IPC fixture', 'Imported fixture').replace('auto_run = false', 'auto_run = true'))
  await page.getByRole('button', { name: 'Import configuration', exact: true }).click()
  await page.getByRole('dialog', { name: 'Confirm import' }).getByRole('button', { name: 'Cancel', exact: true }).last().click()
  assert.equal((await invoke('get_state')).tunnels[0].name, 'Native IPC fixture')
  await page.getByRole('button', { name: 'Import configuration', exact: true }).click()
  await page.getByRole('dialog', { name: 'Confirm import' }).waitFor()
  await page.screenshot({ path: path.join(fixture, 'import-preview-390.png') })
  await page.getByRole('dialog', { name: 'Confirm import' }).getByRole('button', { name: 'Replace configuration', exact: true }).click()
  await page.getByText('Configuration imported.', { exact: true }).waitFor()
  assert.equal((await invoke('get_state')).tunnels[0].name, 'Imported fixture')
  assert.equal((await invoke('get_runtime')).length, 0)
  await expect(loginToggle).toBeChecked()
  assert.equal(await invoke('get_auto_run'), true)
  await loginToggle.click()
  await expect(loginToggle).not.toBeChecked()
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  const toggle = page.getByRole('switch', { name: 'Show default jumper groups' })
  const toggleBox = await toggle.boundingBox()
  assert.ok(toggleBox.width >= 40 && toggleBox.height <= 28, 'Switch must have a normal horizontal track')
  await toggle.uncheck()
  await page.getByLabel('Language').selectOption('zh-CN')
  await expect.poll(() => invoke('get_ui_locale')).toBe('zh-CN')
  assert.equal((await readFile(path.join(configDirectory, 'ui.locale'), 'utf8')).trim(), 'zh-CN')
  await page.getByLabel('\u4e3b\u9898').selectOption('light')
  assert.equal(await page.evaluate(() => document.documentElement.lang), 'zh-CN')
  await page.screenshot({ path: path.join(fixture, 'settings-zh-light-390.png') })
  const chineseNavigation = page.getByRole('navigation', { name: '\u4e3b\u5bfc\u822a' })
  await chineseNavigation.getByRole('button', { name: '\u96a7\u9053', exact: true }).click()
  await expect(page.locator('article .status-stopped')).toHaveText('\u5df2\u505c\u6b62')
  assert.equal(await page.getByRole('navigation', { name: '\u96a7\u9053\u5206\u7ec4' }).getByRole('button', { name: /Local fixture/ }).count(), 0)
  assert.equal(await page.getByRole('navigation', { name: '\u96a7\u9053\u5206\u7ec4' }).getByRole('button', { name: /Fixture group/ }).count(), 1)
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  await chineseNavigation.getByRole('button', { name: '\u8bbe\u7f6e', exact: true }).click()
  await page.getByLabel('\u8bed\u8a00').selectOption('en')
  await expect.poll(() => invoke('get_ui_locale')).toBe('en')
  await page.getByLabel('Theme').selectOption('dark')
  await page.getByRole('switch', { name: 'Show default jumper groups' }).check()
  await navigation.getByRole('button', { name: 'Tunnels', exact: true }).click()
  await invoke('save_group', { group: { id: group, name: 'Fixture group', tunnel_ids: [] } })
  assert.deepEqual((await invoke('get_state')).groups.find(item => item.id === group).tunnel_ids, [])
  await invoke('delete_tunnel', { id: tunnel })
  await invoke('delete_jumper', { id: jumper })
  await invoke('delete_group', { id: group })
  await invoke('delete_group', { id: secondGroup })
  assert.equal((await invoke('get_state')).jumpers.length, 0)
  assert.match(await readFile(config, 'utf8'), /version = 1/)
  const startupJumper = await invoke('save_jumper', { jumper: { ...trusted.jumpers[0], id: 0 } })
  const startupTunnel = await invoke('save_tunnel', { tunnel: { ...trusted.tunnels[0], id: 0, jumper_ids: [startupJumper], auto_start: true } })
  await invoke('start_tunnel', { id: startupTunnel })
  await expect.poll(async () => (await invoke('get_state')).tunnels[0]?.status).toBe('running')
  await navigation.getByRole('button', { name: 'Settings', exact: true }).click()
  const liveDirectory = path.join(fixture, 'live directory')
  const originalBeforeMove = await readFile(config, 'utf8')
  await mkdir(path.join(configDirectory, 'config.root'))
  await assert.rejects(() => invoke('relocate_config', { target: liveDirectory, overwrite: true }))
  assert.equal((await invoke('get_state')).tunnels[0]?.status, 'running')
  assert.equal(await invoke('get_config_path'), config)
  assert.equal(await readFile(config, 'utf8'), originalBeforeMove)
  await rmdir(path.join(configDirectory, 'config.root'))
  await page.getByLabel('Target directory', { exact: true }).fill(liveDirectory)
  await page.getByRole('button', { name: 'Change directory', exact: true }).click()
  let directoryDialog = page.getByRole('dialog', { name: 'Change configuration directory?' })
  await expect(directoryDialog).toBeVisible()
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  await page.screenshot({ path: path.join(fixture, 'directory-confirm-390.png') })
  await directoryDialog.getByRole('button', { name: 'Copy current settings' }).click()
  await expect(directoryDialog).not.toBeVisible()
  const livePath = await invoke('get_config_path')
  assert.ok(livePath.endsWith(path.join('live directory', 'config.toml')))
  await expect.poll(async () => (await invoke('get_state')).tunnels[0]?.status).toBe('running')
  await invoke('set_traffic_monitor', { enabled: false })
  assert.equal(await readFile(config, 'utf8'), originalBeforeMove)
  assert.match(await readFile(livePath, 'utf8'), /traffic_monitor_enabled = false/)
  await writeFile(path.join(configDirectory, 'ui.locale'), 'zh-CN\n')
  await page.getByRole('button', { name: 'Restore default directory' }).click()
  directoryDialog = page.getByRole('dialog', { name: 'Change configuration directory?' })
  await directoryDialog.getByRole('button', { name: 'Use destination settings' }).click()
  await expect(directoryDialog).not.toBeVisible()
  await expect(page.getByRole('navigation', { name: '\u4e3b\u5bfc\u822a' })).toBeVisible()
  assert.equal(await invoke('get_ui_locale'), 'zh-CN')
  assert.equal(await readFile(path.join(configDirectory, 'ui.locale'), 'utf8'), 'zh-CN\n')
  await invoke('set_ui_locale', { locale: 'en' })
  await page.reload()
  await expect(navigation).toBeVisible()
  await navigation.getByRole('button', { name: 'Settings', exact: true }).click()
  assert.equal(await invoke('get_config_path'), config)
  assert.equal((await invoke('get_state')).traffic_monitor_enabled, true)
  assert.ok(!(await invoke('get_activity_logs')).some(entry => entry.message === 'save_jumper'), 'Configuration switch must discard old application activity')
  await expect.poll(async () => (await invoke('get_state')).tunnels[0]?.status).toBe('running')
  await assert.rejects(() => readFile(path.join(configDirectory, 'config.root')), { code: 'ENOENT' })
  assert.match(await readFile(livePath, 'utf8'), /traffic_monitor_enabled = false/)
  const exited = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('Graceful exit timeout')), 10_000)
    child.once('exit', code => { clearTimeout(timeout); resolve(code) })
  })
  const [exitCode] = await Promise.all([exited, page.getByRole('button', { name: 'Quit application', exact: true }).click()])
  assert.equal(exitCode, 0)
  const released = createServer()
  await new Promise((resolve, reject) => { released.once('error', reject); released.listen(localPort, '127.0.0.1', resolve) })
  await new Promise(resolve => released.close(resolve))
  await browser.close()
  // Exercise the legacy pointer on a real restart without touching the source artifacts.
  const redirectedDirectory = path.join(fixture, 'redirected config')
  await mkdir(redirectedDirectory)
  const originalConfig = await readFile(config, 'utf8')
  await writeFile(path.join(redirectedDirectory, 'config.toml'), originalConfig)
  await writeFile(path.join(redirectedDirectory, 'ui.locale'), 'zh-CN\n')
  await writeFile(path.join(configDirectory, 'config.root'), `${redirectedDirectory}\n`)
  const debugReservation = createServer()
  await new Promise(resolve => debugReservation.listen(0, '127.0.0.1', resolve))
  const restartPort = debugReservation.address().port
  await new Promise(resolve => debugReservation.close(resolve))
  child = launchApp(restartPort)
  child.on('error', error => { launchError = error })
  const restartEndpoint = `http://127.0.0.1:${restartPort}`
  await expect.poll(async () => {
    if (launchError) throw launchError
    try { return (await fetch(`${restartEndpoint}/json/version`)).ok } catch { return false }
  }, { timeout: 45_000 }).toBe(true)
  browser = await chromium.connectOverCDP(restartEndpoint)
  page = browser.contexts()[0].pages()[0] ?? await browser.contexts()[0].waitForEvent('page')
  await page.waitForFunction(() => window.__TAURI_INTERNALS__?.invoke)
  await expect.poll(async () => (await invoke('get_state')).tunnels[0]?.status, { timeout: 30_000 }).toBe('running')
  assert.equal(await invoke('get_config_path'), path.join(redirectedDirectory, 'config.toml'))
  assert.equal(await invoke('get_ui_locale'), 'zh-CN')
  await invoke('set_ui_locale', { locale: 'en' })
  assert.equal(await readFile(path.join(redirectedDirectory, 'ui.locale'), 'utf8'), 'en\n')
  await invoke('set_auto_run', { enabled: true })
  const redirectedRegistration = await execFileAsync('reg.exe', ['query', runKey, '/v', autostartName], { windowsHide: true })
  assert.ok(redirectedRegistration.stdout.includes(`--config "${config}"`))
  assert.ok(!redirectedRegistration.stdout.includes('redirected config'))
  await invoke('set_auto_run', { enabled: false })
  assert.equal(await readFile(config, 'utf8'), originalConfig)
  assert.equal((await invoke('get_pending_trust')).length, 0)
  const restartedExit = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('Restarted app exit timeout')), 10_000)
    child.once('exit', code => { clearTimeout(timeout); resolve(code) })
  })
  const [restartExitCode] = await Promise.all([restartedExit, invoke('quit_app')])
  assert.equal(restartExitCode, 0)
  console.log(`Native forms, groups, dialogs, locale/theme, switch sizing, host trust and persistence smoke passed. Evidence: ${fixture}`)
} catch (error) {
  const nativeProcesses = await execFileAsync('powershell.exe', [
    '-NoProfile', '-File', path.join(root, 'scripts/native-process-diagnostics.ps1'),
    '-RootProcessId', String(child.pid),
  ], { windowsHide: true, timeout: 20_000 }).then(result => JSON.parse(result.stdout)).catch(() => null)
  let diagnostic = null
  if (browser) {
    const debugPage = browser.contexts()[0]?.pages()[0]
    if (debugPage) {
      await debugPage.screenshot({ path: path.join(fixture, 'failure.png') }).catch(() => {})
      diagnostic = await debugPage.evaluate(async () => ({ runtime: await window.__TAURI_INTERNALS__.invoke('get_runtime'), logs: await window.__TAURI_INTERNALS__.invoke('get_tunnel_logs', { id: 1 }) })).catch(() => null)
    }
  }
  await writeFile(path.join(fixture, 'failure.json'), JSON.stringify({
    error: String(error), diagnostic, nativeProcesses, lastStartupTransport, output: output.slice(-16000),
    exitCode: child.exitCode, signalCode: child.signalCode,
    webviewRuntime: process.env.WEBVIEW2_BROWSER_EXECUTABLE_FOLDER ?? 'auto-discovery',
    sandboxDisabledForTest: Boolean(sandboxArgument),
    inheritedWebviewArgumentKeys: Object.keys(process.env).filter(key => key.toUpperCase() === 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'),
  }, null, 2))
  console.error(`Native smoke failed. Evidence: ${fixture}`)
  throw error
} finally {
  if (browser) await browser.close()
  child.kill()
  sshFixture?.kill()
  for (const connection of echoConnections) connection.destroy()
  echoServer?.close()
  // This per-run fixture name is never shared with either application registration.
  await execFileAsync('reg.exe', ['delete', runKey, '/v', autostartName, '/f'], { windowsHide: true }).catch(() => {})
}
