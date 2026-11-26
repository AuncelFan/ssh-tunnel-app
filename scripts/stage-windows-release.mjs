import { copyFile, mkdir, mkdtemp, readFile, rename, rm, stat, writeFile } from 'node:fs/promises'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { digest } from './release-assets.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const exec = promisify(execFile)
const git = async (...args) => (await exec('git', args, { cwd: root, windowsHide: true })).stdout.trim()
if (process.platform !== 'win32') throw new Error('This staging command requires Windows')
if (await git('status', '--porcelain')) throw new Error('Commit or otherwise resolve workspace changes before staging a release')
const { version } = JSON.parse(await readFile(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('Expected a numeric three-part application version')
const parent = path.join(root, 'build/bin')
const destination = path.join(parent, version)
await mkdir(parent, { recursive: true })
const staging = await mkdtemp(path.join(parent, `.${version}-`))
try {
  const assets = []
  for (const name of ['ssh-tunnel', 'ssh-tunnel-cli']) {
    const filename = `${name}-${version}.exe`
    const file = path.join(staging, filename)
    await copyFile(path.join(root, 'target/release', `${name}.exe`), file)
    assets.push({ filename, bytes: (await stat(file)).size, sha256: await digest(file) })
  }
  const metadata = { kind: 'release', version, ...assets[0], assets, sourceCommit: await git('rev-parse', 'HEAD'), defaultConfigDirectory: '.ssh-tunnel' }
  await writeFile(path.join(staging, 'SHA256SUMS'), assets.map(a => `${a.sha256}  ${a.filename}\n`).join(''))
  await writeFile(path.join(staging, 'build-info.json'), `${JSON.stringify(metadata, null, 2)}\n`)
  try {
    await rename(staging, destination)
  } catch (error) {
    if (!['EEXIST', 'ENOTEMPTY', 'EPERM', 'EACCES'].includes(error.code)) throw error
    for (const name of [...assets.map(a => a.filename), 'SHA256SUMS', 'build-info.json']) {
      if (await digest(path.join(staging, name)) !== await digest(path.join(destination, name))) {
        throw new Error(`Existing staged release differs at ${name}`)
      }
    }
  }
  console.log(`Staged GUI and CLI in ${destination}`)
} finally {
  await rm(staging, { recursive: true, force: true })
}
