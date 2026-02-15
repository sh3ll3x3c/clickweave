import type { StateCreator } from "zustand";
import type { StoreState } from "./types";

export interface LogSlice {
  logs: string[];

  pushLog: (msg: string) => void;
  clearLogs: () => void;
}

export const createLogSlice: StateCreator<StoreState, [], [], LogSlice> = (set) => ({
  logs: ["Clickweave started"],

  pushLog: (msg) => {
    set((state) => {
      const next = [...state.logs, msg];
      return { logs: next.length > 1000 ? next.slice(-1000) : next };
    });
  },

  clearLogs: () => set({ logs: [] }),
});
