import { invoke } from "@tauri-apps/api/core";
import type {
  ContentView,
  DeviceView,
  HistoryItemView,
  SettingsView,
  StateView,
} from "./types";

export const api = {
  getState: () => invoke<StateView>("get_state"),
  getHistory: (search?: string, status?: string) =>
    invoke<HistoryItemView[]>("get_history", { search, status }),
  getItemContent: (id: string) => invoke<ContentView>("get_item_content", { id }),
  acceptItem: (id: string) => invoke<void>("accept_item", { id }),
  ignoreItem: (id: string) => invoke<void>("ignore_item", { id }),
  deleteItem: (id: string) => invoke<void>("delete_item", { id }),
  clearHistory: () => invoke<void>("clear_history"),
  updateSettings: (settings: SettingsView) =>
    invoke<SettingsView>("update_settings", { settings }),
  pairRequest: (deviceId: string) => invoke<void>("pair_request", { deviceId }),
  respondPairing: (deviceId: string, accept: boolean) =>
    invoke<void>("respond_pairing", { deviceId, accept }),
  cancelPairWait: (deviceId: string) => invoke<void>("cancel_pair_wait", { deviceId }),
  setDeviceFlags: (deviceId: string, sendEnabled?: boolean, receiveEnabled?: boolean) =>
    invoke<DeviceView[]>("set_device_flags", { deviceId, sendEnabled, receiveEnabled }),
  removeDevice: (deviceId: string) => invoke<void>("remove_device", { deviceId }),
  hidePopup: () => invoke<void>("hide_popup"),
};
