#!/usr/bin/env node
// 把草稿 Release 里 latest.json 的下载地址换成普通下载地址。
//
// tauri-action 在 latest.json 里写的是 GitHub API 的资源地址（https://api.github.com/repos/<仓库>/releases/assets/<id>）。
// 不登录访问这种地址，每个 IP 每小时只能请求 60 次；用户共用代理或 VPN 时很容易超限，应用内更新就会下载失败。
// 这里换成 https://github.com/<仓库>/releases/download/<标签>/<文件名>：Release 发布后就能下载，不受这个限制。
//
// 用法：GH_TOKEN=<token> node scripts/updater-links.mjs <owner/repo> <标签> [--dry-run]
// 只改这个标签的草稿 Release（工作流构建出来的就是草稿），已经发布的 Release 不动。--dry-run 只打印结果，不上传。

import { pathToFileURL } from 'node:url'

const api = 'https://api.github.com'

/** 把 manifest 里每个平台的地址换成普通下载地址，返回改了几个；找不到对应文件时报错。 */
export function rewriteLinks(manifest, assets, repo, tag) {
  const byId = new Map(assets.map((asset) => [asset.id, asset]))
  const byName = new Map(assets.map((asset) => [asset.name, asset]))
  const apiPrefix = `${api}/repos/${repo}/releases/assets/`
  let changed = 0
  for (const [platform, entry] of Object.entries(manifest.platforms ?? {})) {
    const asset = entry.url.startsWith(apiPrefix)
      ? byId.get(Number(entry.url.slice(apiPrefix.length)))
      : byName.get(decodeURIComponent(entry.url.split('/').pop() ?? ''))
    if (!asset) throw new Error(`${platform}: no release file for ${entry.url}`)
    const url = `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(asset.name)}`
    if (entry.url !== url) {
      entry.url = url
      changed += 1
    }
  }
  return changed
}

async function github(method, url, { body, accept = 'application/vnd.github+json', contentType } = {}) {
  const headers = { Accept: accept, 'X-GitHub-Api-Version': '2022-11-28' }
  if (process.env.GH_TOKEN) headers.Authorization = `Bearer ${process.env.GH_TOKEN}`
  if (contentType) headers['Content-Type'] = contentType
  // 下载文件时会跳转到另一个域名，fetch 跳转时不会带上 Authorization。
  const response = await fetch(url, { method, headers, body })
  if (!response.ok) throw new Error(`${method} ${url}: HTTP ${response.status} ${await response.text()}`)
  return response
}

async function findDraft(repo, tag) {
  const drafts = []
  for (let page = 1; ; page += 1) {
    const releases = await (await github('GET', `${api}/repos/${repo}/releases?per_page=100&page=${page}`)).json()
    drafts.push(...releases.filter((release) => release.draft && release.tag_name === tag))
    if (releases.length < 100) break
  }
  if (drafts.length === 0) throw new Error(`no draft release for ${tag}`)
  // 正常只有一个；有多个时用最新的（tauri-action 上传到的也是它）。
  drafts.sort((a, b) => b.id - a.id)
  if (drafts.length > 1) console.warn(`found ${drafts.length} drafts for ${tag}, using release ${drafts[0].id}`)
  return drafts[0]
}

async function main() {
  const [repo, tag, flag] = process.argv.slice(2)
  if (!repo || !tag || (flag && flag !== '--dry-run')) {
    console.error('usage: node scripts/updater-links.mjs <owner/repo> <tag> [--dry-run]')
    process.exit(2)
  }
  const release = await findDraft(repo, tag)
  const old = release.assets.find((asset) => asset.name === 'latest.json')
  if (!old) throw new Error(`release ${release.id} has no latest.json`)
  const manifest = await (await github('GET', `${api}/repos/${repo}/releases/assets/${old.id}`, { accept: 'application/octet-stream' })).json()
  const changed = rewriteLinks(manifest, release.assets, repo, tag)
  const body = `${JSON.stringify(manifest, null, 2)}\n`
  if (flag === '--dry-run' || changed === 0) {
    console.log(changed === 0 ? 'latest.json already uses normal download links' : body)
    return
  }
  // 先把旧文件改名腾出名字，新文件上传成功后再删旧的；上传失败时改回原名。
  const rename = (name) => github('PATCH', `${api}/repos/${repo}/releases/assets/${old.id}`, { body: JSON.stringify({ name }), contentType: 'application/json' })
  await rename('latest.json.old')
  try {
    await github('POST', `https://uploads.github.com/repos/${repo}/releases/${release.id}/assets?name=latest.json`, { body, contentType: 'application/json' })
  } catch (error) {
    await rename('latest.json')
    throw error
  }
  await github('DELETE', `${api}/repos/${repo}/releases/assets/${old.id}`)
  console.log(`latest.json in release ${release.id} (${tag}): ${changed} download links now use github.com/${repo}/releases/download/${tag}/`)
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message)
    process.exit(1)
  })
}
