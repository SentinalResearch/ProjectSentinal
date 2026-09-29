import { apiAdapter } from "./api";
import { mockAdapter } from "./mock";
import type { DataAdapter } from "./adapter";
import { dataMode, isMockData } from "./mode";

export const dataAdapter: DataAdapter = dataMode === "mock" ? mockAdapter : apiAdapter;
export { dataMode, isMockData } from "./mode";

export type { DataAdapter, HistoryFilters, LiveEvent } from "./adapter";
export * from "./types";
