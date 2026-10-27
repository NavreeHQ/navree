/** Error bodies (`{statusCode, message, error}`), as the Navree API returns them. */

const REASONS: Record<number, string> = {
  400: "Bad Request",
  401: "Unauthorized",
  403: "Forbidden",
  404: "Not Found",
  409: "Conflict",
  429: "Too Many Requests",
  500: "Internal Server Error",
  502: "Bad Gateway",
  503: "Service Unavailable",
};

export function reasonPhrase(status: number): string {
  return REASONS[status] ?? "Error";
}

export function errorBody(status: number, message?: string) {
  return { statusCode: status, message: message ?? reasonPhrase(status), error: reasonPhrase(status) };
}

export function errorResponse(status: number, message?: string, headers?: HeadersInit): Response {
  return Response.json(errorBody(status, message), { status, headers });
}
