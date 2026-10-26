import { spawnSync } from 'node:child_process'
import { openSync, closeSync, writeFileSync, renameSync, mkdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { resolveUserFolders } from './native-ci-profile.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const output = process.argv[2]
if (!output || !path.isAbsolute(output)) throw new Error('An absolute evidence directory is required')
const log = openSync(path.join(output, 'worker.log'), 'wx')
let result = { exitCode: 1, error: 'Worker did not complete' }
const verified = {}
try {
  const token = spawnSync('powershell.exe', ['-NoProfile', '-Command', '$p = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent()); @{ elevated=$p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator); profile=[Environment]::GetFolderPath("UserProfile", "Create"); local=[Environment]::GetFolderPath("LocalApplicationData", "Create"); roaming=[Environment]::GetFolderPath("ApplicationData", "Create") } | ConvertTo-Json -Compress'], { encoding: 'utf8', windowsHide: true })
  const identity = token.status === 0 ? JSON.parse(token.stdout) : null
  if (!identity || identity.elevated !== false) throw new Error('Native CI worker must run without elevation')
  verified.elevated = false
  const groups = spawnSync('whoami.exe', ['/groups', '/fo', 'csv', '/nh'], { encoding: 'utf8', windowsHide: true })
  const integrityRid = Number(groups.stdout?.match(/S-1-16-(\d+)/)?.[1])
  if (groups.status !== 0 || !Number.isFinite(integrityRid) || integrityRid > 8192) throw new Error(`Native CI worker must have at most medium integrity (observed ${integrityRid})`)
  verified.integrityRid = integrityRid
  // A fresh non-interactive logon may not have Shell Folders registered yet.
  // Derive only from this token's verified profile, never the launcher's environment.
  const resolved = resolveUserFolders(identity)
  Object.assign(identity, resolved.folders)
  verified.initializedAppData = resolved.initializedAppData
  verified.knownFolders = Object.fromEntries(['profile', 'local', 'roaming'].map(key => [key, Boolean(identity[key]) && path.isAbsolute(identity[key])]))
  for (const key of ['profile', 'local', 'roaming']) {
    if (!identity[key] || !path.isAbsolute(identity[key])) throw new Error('Native CI worker has no usable Windows user profile')
    mkdirSync(identity[key], { recursive: true })
  }
  // Alternate-credential launch can inherit the administrator's directory variables.
  const temporary = path.join(identity.local, 'Temp')
  mkdirSync(temporary, { recursive: true })
  const names = new Set(['USERPROFILE', 'HOME', 'LOCALAPPDATA', 'APPDATA', 'TEMP', 'TMP'])
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !names.has(key.toUpperCase())))
  Object.assign(env, { USERPROFILE: identity.profile, HOME: identity.profile, LOCALAPPDATA: identity.local, APPDATA: identity.roaming, TEMP: temporary, TMP: temporary })
  const phases = []
  for (const args of [['--release'], ['--release', '--implicit-config']]) {
    const smoke = spawnSync(process.execPath, [path.join(root, 'frontend/scripts/native-smoke.mjs'), ...args], { cwd: root, env, windowsHide: true, stdio: ['ignore', log, log], timeout: 100_000 })
    phases.push({ implicitConfig: args.includes('--implicit-config'), exitCode: smoke.status ?? 1, error: smoke.error?.message ?? null, signal: smoke.signal })
    if (smoke.status !== 0) break
  }
  result = { ...verified, phases, exitCode: phases.at(-1).exitCode, error: phases.at(-1).error }
} catch (error) {
  result = { ...verified, exitCode: 1, error: String(error) }
} finally {
  closeSync(log)
  const temporary = path.join(output, 'result.pending')
  writeFileSync(temporary, JSON.stringify(result, null, 2), { flag: 'wx' })
  renameSync(temporary, path.join(output, 'result.json'))
}
process.exitCode = result.exitCode
