import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { createReadStream } from 'node:fs'
import { copyFile, mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises'
import { execFileSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
export const platforms = ['windows-x64', 'macos-universal', 'linux-x64']
export function versionFromTag(tag) {
  assert.match(tag ?? '', /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/, 'Expected a stable tag such as v0.1.0')
  assert.equal(tag, tag.trim(), 'Whitespace is not allowed in version tags')
  return tag.slice(1)
}

export async function validateVersion(tag, directory = root) {
  const version = versionFromTag(tag)
  for (const file of ['package.json', 'frontend/package.json', 'src-tauri/tauri.conf.json']) {
    assert.equal(JSON.parse(await readFile(path.join(directory, file), 'utf8')).version, version, `${file} disagrees with tag`)
  }
  const cargo = await readFile(path.join(directory, 'Cargo.toml'), 'utf8')
  const workspace = cargo.match(/\[workspace\.package\]([^]*?)(?:\n\[|$)/)?.[1]
  assert.equal(workspace?.match(/^version\s*=\s*"([^"]+)"/m)?.[1], version, 'Cargo workspace disagrees with tag')
  return version
}

export function assetNames(version, platform) {
  versionFromTag(`v${version}`)
  assert.ok(platforms.includes(platform), 'Unsupported release platform')
  const prefix = `ssh-tunnel-${version}`
  return {
    'windows-x64': [`${prefix}-windows-x64.exe`, `${prefix}-windows-x64-setup.exe`],
    'macos-universal': [`${prefix}-macos-universal.dmg`],
    'linux-x64': [`${prefix}-linux-x64.AppImage`, `${prefix}-linux-x64.deb`],
  }[platform]
}

export async function digest(file) {
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(file)) hash.update(chunk)
  return hash.digest('hex')
}

async function bundleFile(directory, extension, version) {
  const matches = (await readdir(directory)).filter(name => name.endsWith(extension) && name.includes(`_${version}_`))
  assert.equal(matches.length, 1, `Expected exactly one ${extension} bundle for ${version} in ${directory}`)
  return path.join(directory, matches[0])
}

export async function collect(platform, version, commit, output, directory = root) {
  const names = assetNames(version, platform)
  assert.match(commit, /^[a-f0-9]{40}$/)
  const release = path.join(directory, 'target', platform === 'macos-universal' ? 'universal-apple-darwin/release' : 'release')
  let sources
  if (platform === 'windows-x64') {
    sources = [path.join(release, 'ssh-tunnel.exe'), await bundleFile(path.join(release, 'bundle/nsis'), '.exe', version)]
  } else if (platform === 'macos-universal') {
    sources = [await bundleFile(path.join(release, 'bundle/dmg'), '.dmg', version)]
  } else {
    sources = [await bundleFile(path.join(release, 'bundle/appimage'), '.AppImage', version), await bundleFile(path.join(release, 'bundle/deb'), '.deb', version)]
  }
  await mkdir(output, { recursive: true })
  assert.equal((await readdir(output)).length, 0, 'Release staging directory must be empty')
  const assets = []
  for (let index = 0; index < sources.length; index++) {
    const file = path.join(output, names[index])
    await copyFile(sources[index], file)
    const bytes = (await stat(file)).size
    assert.ok(bytes > 0, 'Empty release asset')
    assets.push({ name: names[index], bytes, sha256: await digest(file) })
  }
  const manifest = { version, commit, platform, assets }
  await writeFile(path.join(output, `manifest-${platform}.json`), `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' })
  return manifest
}

export async function verifyAssets(directory, version, commit) {
  assert.match(commit, /^[a-f0-9]{40}$/)
  const allowed = []
  for (const platform of platforms) {
    const manifestName = `manifest-${platform}.json`
    const manifest = JSON.parse(await readFile(path.join(directory, manifestName), 'utf8'))
    assert.equal(manifest.version, version, 'Artifact version mismatch')
    assert.equal(manifest.commit, commit, 'Artifact commit mismatch')
    assert.equal(manifest.platform, platform)
    assert.deepEqual(manifest.assets.map(asset => asset.name), assetNames(version, platform), 'Unexpected asset name or missing asset')
    for (const asset of manifest.assets) {
      const file = path.join(directory, asset.name)
      assert.ok(asset.bytes > 0)
      assert.equal((await stat(file)).size, asset.bytes, 'Artifact size mismatch')
      assert.equal(await digest(file), asset.sha256, 'Artifact checksum mismatch')
      allowed.push(asset.name)
    }
    allowed.push(manifestName)
  }
  assert.deepEqual((await readdir(directory)).sort(), allowed.sort(), 'Unexpected files in release staging')
  const checksums = await Promise.all(allowed.map(async name => `${await digest(path.join(directory, name))}  ${name}`))
  await writeFile(path.join(directory, 'SHA256SUMS'), `${checksums.join('\n')}\n`, { flag: 'wx' })
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const command = process.argv[2]
  const version = await validateVersion(process.env.RELEASE_TAG)
  const commit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', windowsHide: true }).trim()
  if (command === 'validate') {
    console.log(`Validated ${process.env.RELEASE_TAG} at ${commit}`)
  } else if (command === 'collect') {
    assert.equal(execFileSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8', windowsHide: true }).trim(), '', 'Commit the source before collecting release assets')
    await collect(process.argv[3], version, commit, path.join(root, 'target/release-assets'))
  } else if (command === 'verify') {
    await verifyAssets(path.join(root, 'target/release-assets'), version, commit)
  } else {
    throw new Error('Expected validate, collect <platform>, or verify')
  }
}
