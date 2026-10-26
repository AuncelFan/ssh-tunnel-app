import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react'
import { api } from './api'

export type Locale = 'en' | 'zh-CN'
export const zh: Record<string, string> = {
  'Last hour': '近 1 小时',
  'Logs': '日志', 'Log level': '日志级别', 'Activity log': '活动日志', 'Info': '信息', 'Warnings': '警告', 'Errors': '错误', 'Application': '应用', 'Tunnel': '隧道',
  'No log entries.': '暂无日志。', 'The latest 500 events are kept in memory. Switching configuration clears this history.': '最近 500 条事件保存在内存中，切换配置后清空。',
  'Copy error': '复制错误', 'Copied': '已复制', 'Clipboard unavailable': '无法访问剪贴板', 'Duplicate jumper': '复制跳板机',
  'Delete jumper': '删除跳板机', 'Delete tunnel': '删除隧道', 'Reorder groups': '调整分组顺序', 'Confirm server identity': '确认服务器身份', 'Succeeded': '成功',
  'Check whether the local address exists and another application is using the port.': '请确认本地地址有效，并检查端口是否被其他应用占用。',
  'Check the hostname and local DNS settings.': '请检查主机名及本机 DNS 设置。',
  'Check remote bind permissions and whether the remote port is already in use.': '请检查远程监听权限，以及远程端口是否已被占用。',
  'Check the destination from the final jumper, not only from this computer.': '请从最后一台跳板机检查目标的连通性，而不只是从本机检查。',
  'Test connection': '测试连接', 'Diagnose connection': '诊断连接', 'Connection test': '连接测试', 'Close test': '关闭测试', 'Cancel test': '取消测试',
  'Testing connection...': '正在测试连接…', 'Connection test passed': '连接测试通过', 'Connection test failed': '连接测试未通过', 'Elapsed': '耗时', 'Passed': '通过', 'Failed': '失败',
  'Configuration validation': '配置检查', 'Local listening port': '本地监听端口', 'SSH connection and authentication': 'SSH 连接与认证', 'SSH round-trip latency': 'SSH 往返延迟',
  'Remote destination': '远程目标', 'Remote listening port': '远程监听端口', 'Remote listener cleanup': '远程监听清理', 'SOCKS forwarding permission': 'SOCKS 转发权限', 'First-hop DNS': '首跳 DNS', 'First-hop TCP': '首跳 TCP', 'Private key file': '私钥文件',
  'Tests do not save drafts. Confirmed host keys are saved. Stop a running tunnel before testing its listening port.': '测试不会保存草稿，确认信任的主机密钥会保存。测试监听端口前请先停止正在运行的隧道。',
  'Check the host, port, username and ordered jumper chain.': '请检查主机、端口、用户名及跳板链路顺序。',
  'Verify the SSH username and password or authorized public key.': '请核对 SSH 用户名、密码或服务器授权的公钥。',
  'Check network reachability, firewall rules and the configured connection timeout.': '请检查网络连通性、防火墙规则和连接超时设置。',
  'The test was cancelled.': '测试已取消。', 'Verify the displayed fingerprint through a trusted channel before accepting it.': '接受密钥前，请通过可信渠道核对所显示的指纹。',
  'Inspect known_hosts and revocations. Do not bypass a revoked key.': '请检查 known_hosts 和密钥撤销记录，不要绕过已撤销密钥的校验。',
  'Check the private key path, file format and passphrase.': '请检查私钥路径、文件格式和私钥口令。',
  'Check that the SSH agent is running and has the required key loaded.': '请确认 SSH 代理正在运行且已加载所需密钥。',
  'Check the address, port and whether the destination service is listening.': '请检查地址、端口，以及目标服务是否正在监听。',
  'Check SSH server forwarding permissions and destination restrictions.': '请检查 SSH 服务器的转发权限和目标地址限制。',
  'Inspect the failed stage, destination service, firewall and SSH server policy.': '请结合失败阶段，检查目标服务、防火墙及 SSH 服务器策略。',
  'Configuration directory': '配置目录', 'Target directory': '目标目录', 'Leave blank to choose a directory': '留空以选择目录',
  'Change directory': '更改目录', 'Restore default directory': '恢复默认目录', 'Open configuration directory': '打开配置目录',
  'Change configuration directory?': '更改配置目录？', 'Default configuration directory': '默认配置目录',
  'Configuration directory changed.': '配置目录已更改。', 'Use destination settings': '使用目标目录的设置', 'Copy current settings': '复制当前设置',
  'Current tunnels will stop and auto-start tunnels from the selected configuration will start. Original files are retained. Trusted hosts and login startup follow the selected configuration.': '当前隧道将停止，所选配置中的自启动隧道将启动。原目录文件会保留，主机信任和开机启动设置将使用所选配置。',
  'The destination already contains configuration or language settings. Keep them or replace them with the current settings?': '目标目录中已有配置或语言设置。请选择保留这些设置，或用当前设置覆盖。',
  'Import SSH command': '导入 SSH 命令', 'SSH command': 'SSH 命令', 'Parse command': '解析命令', 'Jumper': '跳板机', 'Review authentication': '检查认证设置', 'Port mappings': '端口映射', 'Select mapping': '选择映射', 'Tunnel name': '隧道名称', 'Duplicate mapping': '重复的端口映射', 'Import selected tunnels?': '导入所选隧道？',
  'The command is only parsed, never executed. Review authentication and mappings before importing. Tunnels are saved stopped.': '仅解析命令，不会执行。导入前请检查认证和映射设置，隧道保存后保持停止状态。',
  'Select a new mapping and complete authentication for every hop.': '请选择新的映射，并完善每个跳板机的认证设置。',
  'New jumpers and selected tunnels are saved together. Existing jumpers are reused without changing credentials.': '新跳板机与所选隧道会一起保存。复用现有跳板机时不会修改其凭据。',
  'ssh-import.runtime_option_not_imported': '部分 SSH 运行选项由应用控制，不会直接导入。',
  'Imported': '已导入', 'Duplicates skipped': '已跳过重复项', 'Select all': '全选', 'Clear selection': '清除选择', 'Import selected': '导入所选', 'Import selected aliases?': '导入所选主机别名？',
  'Review the warnings before importing. Existing connections are kept and duplicate endpoints are skipped.': '导入前请检查警告。现有连接不会改变，重复端点会自动跳过。',
  'Import SSH config': '导入 SSH 配置', 'Close importer': '关闭导入窗口', 'SSH config path': 'SSH 配置路径', 'Load aliases': '读取主机别名', 'Review and import': '检查并导入', 'Already imported': '已导入', 'No explicit host aliases found.': '未找到明确的主机别名。',
  'Review each alias before saving. ProxyJump chains must be assembled in the tunnel editor; commands are never executed.': '保存前请检查各别名的设置。ProxyJump 跳板链需要在隧道编辑器中配置；导入时不会执行任何命令。',
  'ssh-import.match_unsupported': 'Match 条件段无法静态解析，已跳过，请核对连接设置。',
  'ssh-import.proxy_jump': '请在隧道编辑器中按此 ProxyJump 顺序配置跳板链。',
  'ssh-import.proxy_command_unsupported': 'ProxyCommand 不会执行，也无法直接导入。',
  'ssh-import.host_key_options_unsupported': '自定义 known_hosts 路径或 HostKeyAlias 尚未导入，请核对服务器指纹。',
  'ssh-import.multiple_identity_files': '此别名包含多个密钥路径，当前仅选择第一个，请检查。',
  'ssh-import.missing_user': '请填写登录用户名。', 'ssh-import.agent_fallback': '未指定密钥文件，将使用 SSH 代理，请检查认证方式。',
  'ssh-import.agent_disabled': '源配置禁用了 SSH 代理，请选择私钥或密码认证。',
  'ssh-import.host_verification_disabled': '源配置关闭了主机校验。建议恢复校验并在连接时确认指纹。',
  'Launch at login': '登录系统时启动应用',
  'Enabled': '启用',
  'The system startup state differs from this preference. Check Windows Task Manager or save the preference again.': '系统启动项状态与此设置不一致。请检查 Windows 任务管理器，或重新保存设置。',
  'Configuration': '配置', 'Active configuration': '当前配置文件', 'Configuration file': '配置文件路径',
  'Leave blank to choose a file': '留空以打开文件选择窗口',
  'Configuration files contain credentials and trusted host keys. Keep exports private and import only trusted files.': '配置文件包含凭据和已信任的主机密钥。请妥善保管导出文件，仅导入可信文件。',
  'Import configuration': '导入配置', 'Export configuration': '导出配置', 'Confirm import': '确认导入', 'Confirm export': '确认导出',
  'This replaces the active configuration, stops current tunnels and starts imported auto-start tunnels. A backup is kept. Imported trust and bypass settings also take effect.': '这将替换当前配置，停止现有隧道并启动导入配置中的自启动隧道。旧配置会备份。导入的密钥信任和跳过校验设置也会生效。',
  'The export includes credentials and trusted host keys. An existing destination file will be replaced with a backup.': '导出文件包含凭据和已信任的主机密钥。如果目标文件已存在，会先备份再替换。',
  'Custom groups': '自定义分组', 'Trusted host keys': '已信任的主机密钥', 'Jumpers bypassing verification': '跳过主机校验的跳板机',
  'Configuration imported.': '配置已导入。', 'Configuration exported.': '配置已导出。', 'Replace configuration': '替换配置',
  'Quit application': '\u9000\u51fa\u5e94\u7528',
  'Traffic monitoring': '\u6d41\u91cf\u76d1\u63a7',
  'Traffic monitoring is disabled.': '\u6d41\u91cf\u76d1\u63a7\u5df2\u5173\u95ed\u3002',
  "Overview": "概览", "Tunnels": "隧道", "Jumpers": "跳板机", "Settings": "设置", "All": "全部",
  "Main navigation": "主导航", "Tunnel groups": "隧道分组", "Manage groups": "管理分组", "New tunnel": "新建隧道", "New jumper": "新建跳板机",
  "Start": "启动", "Stop": "停止", "Start group": "启动本组", "Stop group": "关闭本组", "Details": "详情",
  "Loading configuration...": "正在加载配置…", "No tunnels in this group.": "此分组中没有隧道。", "Add a jumper to create your first tunnel.": "先添加跳板机，再创建隧道。",
  "Total tunnels": "总隧道", "Auto-start": "自启动", "Running": "运行中", "Total traffic": "总流量", "Upload": "上传", "Download": "下载",
  "Edit": "编辑", "Delete": "删除", "Duplicate": "复制", "Cancel": "取消", "Edit tunnel": "编辑隧道", "Edit jumper": "编辑跳板机",
  "Delete tunnel?": "删除此隧道？", "Delete jumper?": "删除此跳板机？", "No description": "暂无描述", "Close details": "关闭详情", "Dismiss error": "关闭错误通知",
  "Show default jumper groups": "显示按跳板机分类的默认分组", "Default groups follow the first jumper. Custom groups remain available.": "默认分组按首个跳板机分类，不影响自定义分组。",
  "Name": "名称", "Host": "主机", "SSH port": "SSH 端口", "User": "用户名", "Authentication": "认证方式", "Private key": "私钥", "Password": "密码", "SSH agent": "SSH 代理",
  "Private key path": "私钥路径", "Key passphrase": "私钥口令", "Agent socket / named pipe": "代理套接字 / 命名管道", "System default": "系统默认",
  "Connection timeout (ms)": "连接超时（毫秒）", "Keepalive interval (ms)": "保活间隔（毫秒）", "Host-key algorithms": "主机密钥算法",
  "Skip host verification (unsafe)": "跳过主机校验（不安全）", "Server identity will not be checked. Prefer confirming the server fingerprint when prompted.": "跳过后将不验证服务器身份。建议保留校验，并在提示时核对服务器指纹。",
  "Notes": "备注", "Save jumper": "保存跳板机", "Close editor": "关闭编辑窗口", "Type": "类型", "L - Local": "L - 本地转发", "R - Remote": "R - 远程转发", "S - SOCKS5": "S - SOCKS5 代理",
  "Local host": "本地主机", "Local port": "本地端口", "Remote host": "远程主机", "Remote port": "远程端口", "Local endpoint": "本地端点", "Remote endpoint": "远程端点",
  "Jumper chain": "跳板链", "Connections follow this order. The first jumper determines the default group.": "连接将按此顺序建立，首个跳板机决定默认分组。", "Add jumper": "添加跳板机", "Select a jumper": "选择跳板机", "Remove": "移除",
  "Start automatically": "自动启动", "Reconnect policy": "重连策略", "Disabled": "禁用", "Fixed interval": "固定间隔", "Exponential backoff": "指数退避", "Legacy": "兼容策略",
  "Retry interval (seconds)": "重试间隔（秒）", "Maximum retries (0 = unlimited)": "最大重试次数（0 表示无限）", "Description": "描述", "Save tunnel": "保存隧道", "Select at least one jumper.": "请至少选择一个跳板机。",
  "Close group manager": "关闭分组管理", "Custom groups are filters. A tunnel can belong to more than one custom group. Jumper groups are managed automatically.": "自定义分组是筛选器，同一隧道可以属于多个分组。按跳板机分类的默认分组由系统维护。",
  "New group": "新建分组", "Group name": "分组名称", "Delete group": "删除分组", "Save group": "保存分组", "Tunnel log": "隧道日志",
  "Server identity changed": "服务器身份发生变化", "Trust this server?": "是否信任此服务器？", "Cancel connection": "取消连接",
  "The received key differs from the saved key. This can mean a server reinstall or an intercepted connection.": "收到的密钥与已保存的密钥不同，可能是服务器重装，也可能是连接遭到拦截。",
  "This server key has not been trusted yet.": "尚未信任此服务器密钥。", "Verify its SHA256 fingerprint through a separate trusted channel before continuing.": "继续前，请通过独立的可信渠道核对 SHA256 指纹。",
  "Previously trusted": "原有可信指纹", "Received fingerprint": "本次收到的指纹", "Only this endpoint and key will be trusted. SSH host verification remains enabled.": "仅信任此端点的当前密钥，SSH 主机校验仍保持启用。",
  "Replace saved key and reconnect": "替换已保存的密钥并重连", "Trust key and connect": "信任密钥并连接",
  "Language": "语言", "Theme": "主题", "Dark": "深色", "Light": "浅色", "Copy suffix": "副本",
  "status.stopped": "已停止", "status.running": "运行中", "status.connecting": "连接中", "status.reconnecting": "重连中", "status.awaiting_trust": "等待信任确认", "status.error": "连接错误",
  "mode.local": "本地转发", "mode.remote": "远程转发", "mode.dynamic": "SOCKS5 代理", "Move up": "上移", "Move down": "下移",
  "policy.fixed": "固定间隔", "policy.exponential": "指数退避", "policy.disabled": "禁用", "policy.legacy": "兼容策略", "event.status": "状态", "event.error": "错误", "event.host_key": "主机密钥",
}

const en: Record<string, string> = {
  'ssh-import.runtime_option_not_imported': 'Some SSH runtime options are controlled by the app and are not imported directly.',
  'ssh-import.match_unsupported': 'Conditional Match sections were skipped. Review connection settings.',
  'ssh-import.proxy_jump': 'Assemble this ProxyJump chain in the tunnel editor before connecting.',
  'ssh-import.proxy_command_unsupported': 'ProxyCommand is not executed and cannot be imported directly.',
  'ssh-import.host_key_options_unsupported': 'Custom known_hosts paths or HostKeyAlias are not imported. Verify the server fingerprint.',
  'ssh-import.multiple_identity_files': 'Only the first of multiple identity files is selected. Review the key path.',
  'ssh-import.missing_user': 'Enter the SSH login user.', 'ssh-import.agent_fallback': 'No identity file is specified; SSH agent is selected. Review authentication.',
  'ssh-import.agent_disabled': 'The source disables SSH agent. Select a private key or password instead.',
  'ssh-import.host_verification_disabled': 'The source disables host verification. Prefer enabling it and confirming the fingerprint.',
  'policy.fixed': 'Fixed interval', 'policy.exponential': 'Exponential backoff', 'policy.disabled': 'Disabled', 'policy.legacy': 'Legacy', 'event.status': 'Status', 'event.error': 'Error', 'event.host_key': 'Host key',
  'Copy suffix': 'copy', 'status.stopped': 'Stopped', 'status.running': 'Running', 'status.connecting': 'Connecting', 'status.reconnecting': 'Reconnecting', 'status.awaiting_trust': 'Awaiting trust', 'status.error': 'Error', 'mode.local': 'Local forwarding', 'mode.remote': 'Remote forwarding', 'mode.dynamic': 'SOCKS5',
}
export const translate = (locale: Locale, key: string) => (locale === 'zh-CN' ? zh[key] : en[key]) ?? key
const Context = createContext<{ locale: Locale; setLocale: (locale: Locale) => Promise<void>; reloadLocale: () => Promise<void>; t: (key: string) => string }>({ locale: 'en', setLocale: async () => {}, reloadLocale: async () => {}, t: key => translate('en', key) })
export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, updateLocale] = useState<Locale>(() => localStorage.getItem('ssh-tunnel.locale') === 'zh-CN' ? 'zh-CN' : 'en')
  const [error, setError] = useState('')
  const version = useRef(0)
  useEffect(() => {
    let disposed = false
    const initialVersion = version.current
    void api.getUILocale().then(value => {
      if (!disposed && version.current === initialVersion) updateLocale(value === 'zh-CN' ? 'zh-CN' : 'en')
    }).catch(error => { if (!disposed) setError(String(error)) })
    return () => { disposed = true }
  }, [])
  async function setLocale(next: Locale) {
    const sequence = ++version.current
    await api.setUILocale(next)
    if (sequence === version.current) { updateLocale(next); setError('') }
  }
  async function reloadLocale() {
    const sequence = ++version.current
    const next = await api.getUILocale()
    if (sequence === version.current) { updateLocale(next === 'zh-CN' ? 'zh-CN' : 'en'); setError('') }
  }
  useEffect(() => { localStorage.setItem('ssh-tunnel.locale', locale); document.documentElement.lang = locale }, [locale])
  return <Context.Provider value={{ locale, setLocale, reloadLocale, t: key => translate(locale, key) }}>{children}{error && <aside className="toast" role="alert">{error}</aside>}</Context.Provider>
}
export const useLocale = () => useContext(Context)
