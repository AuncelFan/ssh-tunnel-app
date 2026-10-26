import { createHash } from 'node:crypto'
import { createReadStream, constants } from 'node:fs'
import { copyFile, mkdir, readFile, stat, writeFile } from 'node:fs/promises'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const exec = promisify(execFile)
const git = async (...args) => (await exec('git', args, { cwd: root, windowsHide: true })).stdout.trim()
if (process.platform !== 'win32') throw new Error('This staging command requires Windows')
if (await git('status', '--porcelain')) throw new Error('Commit or otherwise resolve workspace changes before staging a release')
const config = JSON.parse(await readFile(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
if (!/^\d+\.\d+\.\d+$/.test(config.version)) throw new Error('Expected a numeric three-part application version')
const source = path.join(root, 'target/release/ssh-tunnel.exe')
const digest = async file => {
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(file)) hash.update(chunk)
  return hash.digest('hex')
}
const sha256 = await digest(source)
const directory = path.join(root, 'build/bin', config.version)
await mkdir(directory, { recursive: true })
const filename = `ssh-tunnel-${config.version}.exe`
const destination = path.join(directory, filename)
try { await copyFile(source, destination, constants.COPYFILE_EXCL) }
catch (error) { if (error.code !== 'EEXIST') throw error }
if (await digest(destination) !== sha256) throw new Error('The existing staged executable has different contents; no file was overwritten')
const metadata = {
  kind: 'release', version: config.version, filename, sha256,
  bytes: (await stat(destination)).size, sourceCommit: await git('rev-parse', 'HEAD'),
  defaultConfigDirectory: '.ssh-tunnel',
}
for (const [name, content] of [
  ['SHA256SUMS', `${sha256}  ${filename}\n`],
  ['build-info.json', `${JSON.stringify(metadata, null, 2)}\n`],
]) {
  try { await writeFile(path.join(directory, name), content, { flag: 'wx' }) }
  catch (error) { if (error.code !== 'EEXIST') throw error }
}
console.log(`Staged release: ${destination}\nSHA256 ${sha256}`)
