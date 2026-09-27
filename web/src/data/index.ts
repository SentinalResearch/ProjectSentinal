import { apiAdapter } from "./api";
import { mockAdapter } from "./mock";
import type { DataAdapter } from "./adapter";

const mode = import.meta.env.VITE_DATA === "api" ? "api" : "mock";

export const dataAdapter: DataAdapter = mode === "api" ? apiAdapter : mockAdapter;
export const isMockData = mode === "mock";

export type { DataAdapter, HistoryFilters, LiveEvent } from "./adapter";
export * from "./types";
