(function installSentinalPushSafety(scope) {
  const encoder = new TextEncoder();

  function boundedSafeText(value, maximumBytes) {
    return (
      typeof value === "string" &&
      value.length > 0 &&
      encoder.encode(value).length <= maximumBytes &&
      !/[\u0000-\u001f\u007f]/u.test(value)
    );
  }

  function safeNotificationRoute(value, origin) {
    if (typeof value !== "string" || encoder.encode(value).length > 2048 || !value.startsWith("/app/sessions/")) return null;
    if (value.startsWith("//") || value.includes("\\") || value.includes("#") || value.includes("?")) return null;
    if (/%(?:2f|5c|2e)/iu.test(value)) return null;
    try {
      const parsed = new URL(value, origin);
      return parsed.origin === origin && parsed.pathname.startsWith("/app/sessions/") ? parsed.pathname : null;
    } catch {
      return null;
    }
  }

  function safePushPayload(raw, origin) {
    if (typeof raw !== "string" || encoder.encode(raw).length > 2048) return null;
    let payload;
    try {
      payload = JSON.parse(raw);
    } catch {
      return null;
    }
    const route = safeNotificationRoute(payload?.route, origin);
    if (
      payload?.v !== 1 ||
      !route ||
      !boundedSafeText(payload.title, 100) ||
      !boundedSafeText(payload.body, 240) ||
      !boundedSafeText(payload.category, 32)
    ) return null;
    return { title: payload.title, body: payload.body, category: payload.category, route };
  }

  scope.SentinalPushSafety = Object.freeze({ safeNotificationRoute, safePushPayload });
})(self);
