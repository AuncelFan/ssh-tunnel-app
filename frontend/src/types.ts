export interface Jumper {
  id: number; name: string; host: string; port: number; user: string;
  auth_type: 'password' | 'ssh_key' | 'ssh_agent'; key_path: string;
  agent_socket_path: string; password: string; bypass_host_verification: boolean;
  keep_alive_interval_ms: number; timeout_ms: number; host_key_algorithms: string; notes: string;
}
export interface Tunnel {
  id: number; name: string; group_id: number; mode: 'local' | 'remote' | 'dynamic';
  jumper_ids: number[]; local_host: string; local_port: number;
  remote_host: string; remote_port: number; auto_start: boolean;
  status: string; last_error: string; description: string;
  reconnect_mode: string; reconnect_interval_seconds: number; reconnect_max_attempts: number;
}
export interface Group { id: number; name: string; tunnel_ids: number[] }
export interface Config {
  version: number; jumpers: Jumper[]; groups: Group[]; tunnels: Tunnel[];
  auto_run: boolean; traffic_monitor_enabled: boolean;
  trusted_hosts: { host: string; port: number; public_key: string }[];
}
export interface Filter { key: string; name: string; kind: 'all' | 'jumper' | 'custom'; tunnels: Tunnel[] }
export interface HostKeyChallenge { jumper_id: number; host: string; port: number; previous_fingerprint: string | null; fingerprint: string; public_key: string; authority_fingerprint?: string | null }
export interface RuntimeEvent { timestamp_ms?: number; tunnel_id: number; generation: number; kind: string; message: string; challenge: HostKeyChallenge | null }
export interface TunnelRuntime { tunnel_id: number; generation: number; status: string; last_error: string; retry_attempt: number; traffic: { uploaded: number; downloaded: number }; traffic_last_hour: { uploaded: number; downloaded: number } }
