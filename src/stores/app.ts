import { create } from "zustand";
import { api } from "../lib/ipc";
import type { DeviceView, HistoryItemView, ItemStatus, OwnInfo, PairingEvent, SettingsView } from "../lib/types";

export type Page = "history" | "devices" | "settings";

export interface Toast {
  id: number;
  kind: "ok" | "err" | "info";
  text: string;
}

let toastSeq = 1;

interface AppStore {
  page: Page;
  own: OwnInfo | null;
  settings: SettingsView | null;
  devices: DeviceView[];
  history: HistoryItemView[];
  filter: "all" | ItemStatus;
  search: string;
  incomingPairing: PairingEvent | null;
  waitingPairing: PairingEvent | null;
  toasts: Toast[];

  setPage: (p: Page) => void;
  setFilter: (f: "all" | ItemStatus) => void;
  setSearch: (s: string) => void;
  setIncomingPairing: (e: PairingEvent | null) => void;
  setWaitingPairing: (e: PairingEvent | null) => void;
  refreshState: () => Promise<void>;
  refreshHistory: () => Promise<void>;
  toast: (kind: Toast["kind"], text: string) => void;
  dismissToast: (id: number) => void;
}

export const useApp = create<AppStore>((set, get) => ({
  page: "history",
  own: null,
  settings: null,
  devices: [],
  history: [],
  filter: "all",
  search: "",
  incomingPairing: null,
  waitingPairing: null,
  toasts: [],

  setPage: (page) => set({ page }),
  setFilter: (filter) => set({ filter }),
  setSearch: (search) => set({ search }),
  setIncomingPairing: (incomingPairing) => set({ incomingPairing }),
  setWaitingPairing: (waitingPairing) => set({ waitingPairing }),

  refreshState: async () => {
    try {
      const s = await api.getState();
      set({ own: s.own, settings: s.settings, devices: s.devices });
    } catch (e) {
      console.error("refreshState failed", e);
    }
  },
  refreshHistory: async () => {
    try {
      const { search, filter } = get();
      const list = await api.getHistory(search || undefined, filter);
      set({ history: list });
    } catch (e) {
      console.error("refreshHistory failed", e);
    }
  },

  toast: (kind, text) => {
    const id = toastSeq++;
    set((s) => ({ toasts: [...s.toasts, { id, kind, text }] }));
    setTimeout(() => get().dismissToast(id), 3200);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
