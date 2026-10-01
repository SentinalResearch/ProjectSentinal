/** A real adopted writer process and the `sentinal adopt` output. */
export interface AdoptedWriter {
  output: string;
  stop(): void;
}

export function adoptTrainingWriter(title: string, epochs: number): AdoptedWriter;
