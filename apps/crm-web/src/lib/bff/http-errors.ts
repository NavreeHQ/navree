/** Error bodies (`{statusCode, message, error, code}`), as the Navree API returns them. */

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

/** A body in Navree's error shape; `code` is the machine-readable code the UI branches on. */
export function errorBody(status: number, code: string, message?: string) {
  return { statusCode: status, message: message ?? reasonPhrase(status), error: reasonPhrase(status), code };
}

export function errorResponse(status: number, code: string, message?: string, headers?: HeadersInit): Response {
  return Response.json(errorBody(status, code, message), { status, headers });
}
