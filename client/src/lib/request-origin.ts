type HeaderGetter = {
  headers: {
    get(name: string): string | null;
  };
  url: string;
};

function firstHeaderValue(value: string | null): string | null {
  const first = value?.split(",")[0]?.trim();
  return first ? first : null;
}

export function getExternalOrigin(request: HeaderGetter): string {
  const forwardedHost = firstHeaderValue(request.headers.get("x-forwarded-host"));
  const forwardedProto = firstHeaderValue(request.headers.get("x-forwarded-proto"));

  if (forwardedHost) {
    return `${forwardedProto ?? "https"}://${forwardedHost}`;
  }

  const host = firstHeaderValue(request.headers.get("host"));
  if (host) {
    const protocol = forwardedProto ?? (host.startsWith("localhost") || host.startsWith("127.0.0.1") ? "http" : "https");
    return `${protocol}://${host}`;
  }

  return new URL(request.url).origin;
}

export function buildExternalUrl(path: string, request: HeaderGetter): URL {
  return new URL(path, getExternalOrigin(request));
}
