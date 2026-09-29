/** Data source selected for this frontend build. */
export type DataMode = "api" | "mock";

/** Select fixtures only when mock mode was requested explicitly. */
export function selectDataMode(value: string | undefined): DataMode {
  return value === "mock" ? "mock" : "api";
}

/** Data source selected from Vite's build environment. */
export const dataMode = selectDataMode(import.meta.env.VITE_DATA);

/** Whether this build intentionally uses demonstration fixtures. */
export const isMockData = dataMode === "mock";
