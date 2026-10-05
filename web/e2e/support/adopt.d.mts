/** A real adopted writer process and the `sentinal adopt` output. */
export interface AdoptedWriter {
  output: string;
  stop(): void;
}

export function adoptTrainingWriter(title: string, epochs: number): AdoptedWriter;

/** Result of one `sentinal` run. */
export interface SentinalRun {
  status: number;
  stdout: string;
  stderr: string;
}

export function sentinal(socket: string, args: string[], input?: string): SentinalRun;

export function linkSecrets(root: string): string[];

export function createWorkerSession(
  url: string,
  machineToken: string,
  title: string,
): Promise<{ id: number; machine_id: string }>;

/** Send one authenticated daemon request outside the browser security boundary. */
export function daemonRequest(
  url: string,
  machineToken: string,
  payload: Record<string, unknown>,
  responseType: string,
): Promise<Record<string, unknown>>;

/** Emit a real shell-hook Command through one daemon's private socket. */
export function reportCommand(
  socket: string,
  title: string,
  exitCode?: number,
): void;

export function e2eEnv(name: string): string;
