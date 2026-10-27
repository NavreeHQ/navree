/** Error bodies in Navree's JSON shape (`{statusCode, message, error, code}`), for the BFF's own refusals. */

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

/** The code of a status without a more specific one: the reason phrase in snake case. */
function statusCode(status: number): string {
  if (status === 401) return "unauthenticated";
  if (status === 500) return "internal_error";
  return reasonPhrase(status).toLowerCase().replace(/[^a-z0-9]+/g, "_");
}

export interface ErrorBody {
  statusCode: number;
  message: string;
  error: string;
  code: string;
}

export function errorBody(status: number, message?: string, code?: string): ErrorBody {
  return {
    statusCode: status,
    message: message ?? reasonPhrase(status),
    error: reasonPhrase(status),
    code: code ?? statusCode(status),
  };
}

export function errorResponse(status: number, message?: string, code?: string, headers?: HeadersInit): Response {
  return Response.json(errorBody(status, message, code), { status, headers });
}
