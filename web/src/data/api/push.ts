import type { LinkFailure } from "../../protocol/LinkFailure";
import type { NotificationJournalEntry } from "../../protocol/NotificationJournalEntry";
import type { PushProviderMode } from "../../protocol/PushProviderMode";
import type { PushPublicKey } from "../../protocol/PushPublicKey";
import type { PushSubscriptionInfo } from "../../protocol/PushSubscriptionInfo";
import type { PushSubscriptionList } from "../../protocol/PushSubscriptionList";
import type { PushSubscriptionRequest } from "../../protocol/PushSubscriptionRequest";

export class PushApiError extends Error {}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`/api/v1/push${path}`, {
    ...init,
    credentials: "same-origin",
    headers: init.body ? { "content-type": "application/json" } : undefined,
  });
  if (response.status === 204) return undefined as T;
  const value = (await response.json()) as T | LinkFailure;
  if (!response.ok) {
    const failure = value as LinkFailure;
    throw new PushApiError(failure.message ?? `Push request failed with HTTP ${response.status}`);
  }
  return value as T;
}

export const pushApi = {
  publicKey: () => request<PushPublicKey>("/public-key"),
  history: () => request<NotificationJournalEntry[]>("/deliveries"),
  list: () => request<PushSubscriptionList>("/subscriptions"),
  subscribe: (value: PushSubscriptionRequest) =>
    request<PushSubscriptionInfo>("/subscriptions", {
      method: "POST",
      body: JSON.stringify(value),
    }),
  unsubscribe: (subscriptionId: string) =>
    request<void>(`/subscriptions/${encodeURIComponent(subscriptionId)}`, { method: "DELETE" }),
  setProvider: (mode: PushProviderMode) =>
    request<PushSubscriptionList>("/provider", {
      method: "PUT",
      body: JSON.stringify({ mode }),
    }),
};

function decodeBase64Url(value: string): Uint8Array<ArrayBuffer> {
  const padded = `${value}${"=".repeat((4 - (value.length % 4)) % 4)}`;
  const binary = atob(padded.replace(/-/g, "+").replace(/_/g, "/"));
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

/** Subscribe only after an explicit owner gesture and persist it at the control. */
export async function subscribeCurrentBrowser(deviceLabel: string): Promise<PushSubscriptionInfo> {
  if (!("serviceWorker" in navigator) || !("PushManager" in window)) {
    throw new PushApiError("Web Push is not supported by this browser.");
  }
  const permission = await Notification.requestPermission();
  if (permission !== "granted") throw new PushApiError("Notification permission was not granted.");
  const registration = await navigator.serviceWorker.ready;
  const key = await pushApi.publicKey();
  let subscription = await registration.pushManager.getSubscription();
  if (subscription === null) {
    subscription = await registration.pushManager.subscribe({
      userVisibleOnly: true,
      applicationServerKey: decodeBase64Url(key.public_key),
    });
  }
  const json = subscription.toJSON();
  if (!json.endpoint || !json.keys?.p256dh || !json.keys.auth) {
    throw new PushApiError("The browser returned an incomplete Push subscription.");
  }
  return pushApi.subscribe({
    device_label: deviceLabel,
    endpoint: json.endpoint,
    expiration_time: subscription.expirationTime,
    keys: { p256dh: json.keys.p256dh, auth: json.keys.auth },
  });
}
