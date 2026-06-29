import { create } from "zustand";

export interface LogEntry {
  id: string;
  timestamp: string;
  level: "info" | "warn" | "error" | "debug";
  message: string;
  source?: string;
}

interface LogFilter {
  level?: string;
  search?: string;
}

interface LogStore {
  logs: LogEntry[];
  maxLogs: number;
  isStreaming: boolean;
  filter: LogFilter;
  addLog: (entry: LogEntry) => void;
  addLogs: (entries: LogEntry[]) => void;
  setStreaming: (val: boolean) => void;
  setFilter: (filter: Partial<LogFilter>) => void;
  clearLogs: () => void;
}

export const useLogStore = create<LogStore>((set, get) => ({
  logs: [],
  maxLogs: 10_000,
  isStreaming: false,
  filter: {},

  addLog: (entry) =>
    set((state) => ({
      logs: [...state.logs.slice(-(state.maxLogs - 1)), entry],
    })),

  addLogs: (entries) =>
    set((state) => ({
      logs: [
        ...state.logs.slice(-(state.maxLogs - entries.length)),
        ...entries,
      ],
    })),

  setStreaming: (val) => set({ isStreaming: val }),

  setFilter: (filter) =>
    set((state) => ({ filter: { ...state.filter, ...filter } })),

  clearLogs: () => set({ logs: [] }),
}));
