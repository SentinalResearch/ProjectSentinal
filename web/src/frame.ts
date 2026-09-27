const HEADER_BYTES = 8;

/** Encode terminal bytes with the protocol's big-endian session header. */
export function encodeTerminalFrame(
  sessionId: number,
  payload: Uint8Array,
): Uint8Array {
  if (!Number.isSafeInteger(sessionId) || sessionId < 0) {
    throw new Error(`session id ${sessionId} is not a non-negative safe integer`);
  }
  const frame = new Uint8Array(HEADER_BYTES + payload.byteLength);
  new DataView(frame.buffer).setBigUint64(0, BigInt(sessionId), false);
  frame.set(payload, HEADER_BYTES);
  return frame;
}

/** Decode a terminal frame into its session id and raw terminal bytes. */
export function decodeTerminalFrame(frame: Uint8Array): {
  sessionId: number;
  payload: Uint8Array;
} {
  if (frame.byteLength < HEADER_BYTES) {
    throw new Error(
      `terminal frame is ${frame.byteLength} bytes; expected at least ${HEADER_BYTES}`,
    );
  }
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  const id = view.getBigUint64(0, false);
  if (id > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error(`session id ${id} exceeds JavaScript's safe integer range`);
  }
  return {
    sessionId: Number(id),
    payload: frame.slice(HEADER_BYTES),
  };
}
