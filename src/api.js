import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const api = {
  login: (usernameOrEmail, password) => invoke('auth_login', { usernameOrEmail, password }),
  logout: () => invoke('auth_logout'),

  connect: () => invoke('vpn_connect'),
  disconnect: () => invoke('vpn_disconnect'),
  getStatus: () => invoke('vpn_get_status'),
  onStatus: (cb) => listen('vpn:status', (event) => cb(event.payload)),
  onLog: (cb) => listen('vpn:log', (event) => cb(event.payload)),

  getSplitTunnelConfig: () => invoke('split_tunnel_get_config'),
  setSplitTunnelConfig: (config) => invoke('split_tunnel_set_config', { config }),
  listCandidateApps: () => invoke('split_tunnel_list_candidate_apps'),

  listDevices: () => invoke('devices_list'),
  enrollDevice: (deviceName, platform) => invoke('devices_enroll', { deviceName, platform }),
  revokeDevice: (deviceId) => invoke('devices_revoke', { deviceId }),
  getSubscription: () => invoke('subscription_me'),
};
