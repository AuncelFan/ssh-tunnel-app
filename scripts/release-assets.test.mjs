import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { assetNames, collect, digest, platforms, validateVersion, verifyAssets, versionFromTag } from './release-assets.mjs'

const commit = 'a'.repeat(40)
async function fixture(t) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'ssh-tunnel-release-'))
  t.after(() => rm(dir, { recursive: true, force: true }))
  return dir
}
test('only stable version tags are accepted', () => {
  assert.equal(versionFromTag('v0.1.0'), '0.1.0')
  for (const tag of ['main', 'v0.1', 'v01.1.0', 'v0.1.0-beta', 'v0.1.0\n', 'v0.1.0;echo test']) {
    assert.throws(() => versionFromTag(tag))
  }
})
test('all application version declarations must agree with the tag', async t => {
  const dir = await fixture(t)
  for (const file of ['package.json', 'frontend/package.json', 'src-tauri/tauri.conf.json']) {
    await mkdir(path.dirname(path.join(dir, file)), { recursive: true })
    await writeFile(path.join(dir, file), JSON.stringify({ version: '0.1.0' }))
  }
  await writeFile(path.join(dir, 'Cargo.toml'), '[workspace.package]\nversion = "0.1.0"\n')
  assert.equal(await validateVersion('v0.1.0', dir), '0.1.0')
  await writeFile(path.join(dir, 'Cargo.toml'), '[workspace.package]\nversion = "0.2.0"\n')
  await assert.rejects(validateVersion('v0.1.0', dir))
})
async function manifests(dir) {
  for (const platform of platforms) {
    const assets = []
    for (const name of assetNames('0.1.0', platform)) {
      await writeFile(path.join(dir, name), 'test artifact')
      assets.push({ name, bytes: 13, sha256: await digest(path.join(dir, name)) })
    }
    await writeFile(path.join(dir, `manifest-${platform}.json`), JSON.stringify({ version: '0.1.0', commit, platform, assets }))
  }
}
test('complete matching artifacts generate checksums for every attachment', async t => {
  const dir = await fixture(t)
  await manifests(dir)
  await verifyAssets(dir, '0.1.0', commit)
  assert.equal((await readFile(path.join(dir, 'SHA256SUMS'), 'utf8')).trim().split('\n').length, 8)
})
for (const fault of ['missing-platform', 'tampered', 'wrong-commit', 'unexpected-file', 'unsafe-name']) {
  test(`publication rejects ${fault}`, async t => {
    const dir = await fixture(t)
    await manifests(dir)
    const file = path.join(dir, 'manifest-windows-x64.json')
    if (fault === 'missing-platform') await rm(file)
    if (fault === 'tampered') await writeFile(path.join(dir, assetNames('0.1.0', 'windows-x64')[0]), 'bad artifact!')
    if (fault === 'unexpected-file') await writeFile(path.join(dir, 'config.toml'), 'must not be published')
    if (fault === 'wrong-commit' || fault === 'unsafe-name') {
      const manifest = JSON.parse(await readFile(file, 'utf8'))
      if (fault === 'wrong-commit') manifest.commit = 'b'.repeat(40)
      else manifest.assets[0].name = '../secret'
      await writeFile(file, JSON.stringify(manifest))
    }
    await assert.rejects(verifyAssets(dir, '0.1.0', commit))
  })
}
test('Windows collection names assets explicitly and refuses stale output', async t => {
  const dir = await fixture(t)
  const release = path.join(dir, 'target/release')
  await mkdir(path.join(release, 'bundle/nsis'), { recursive: true })
  await writeFile(path.join(release, 'ssh-tunnel.exe'), 'portable')
  await writeFile(path.join(release, 'bundle/nsis/ssh-tunnel_0.1.0_x64-setup.exe'), 'installer')
  const output = path.join(dir, 'assets')
  const manifest = await collect('windows-x64', '0.1.0', commit, output, dir)
  assert.deepEqual(manifest.assets.map(asset => asset.name), assetNames('0.1.0', 'windows-x64'))
  await assert.rejects(collect('windows-x64', '0.1.0', commit, output, dir))
})

for (const [platform, packages] of [
  ['macos-universal', ['universal-apple-darwin/release/bundle/dmg/ssh-tunnel_0.1.0_universal.dmg']],
  ['linux-x64', ['release/bundle/appimage/ssh-tunnel_0.1.0_amd64.AppImage', 'release/bundle/deb/ssh-tunnel_0.1.0_amd64.deb']],
]) {
  test(`${platform} collection selects only the current version's packages`, async t => {
    const dir = await fixture(t)
    for (const relative of packages) {
      const file = path.join(dir, 'target', relative)
      await mkdir(path.dirname(file), { recursive: true })
      await writeFile(file, 'package')
      await writeFile(file.replace('_0.1.0_', '_0.0.9_'), 'older package')
    }
    const manifest = await collect(platform, '0.1.0', commit, path.join(dir, 'assets'), dir)
    assert.deepEqual(manifest.assets.map(asset => asset.name), assetNames('0.1.0', platform))
  })
}
