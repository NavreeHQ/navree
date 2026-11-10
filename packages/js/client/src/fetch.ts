/**
 * The typed fetch core: `createClient<paths>()` calls the routes of the `paths`
 * type that `navree openapi:client` generates, and infers the path, query and
 * header parameters, the request body and the data and error of the answer from
 * it. It has no dependencies; `createNavreeClient` in `client.ts` adds Navree's
 * conventions on top.
 *
 * ```ts
 * import { createClient } from "@navree/client/fetch";
 * import type { paths } from "./schema.js";
 *
 * const api = createClient<paths>({ baseUrl: "https://api.example.com" });
 * const { data, error } = await api.GET("/api/v1/deals/{id}", { params: { path: { id } } });
 * ```
 */

/* oxlint-disable typescript/no-explicit-any, typescript/no-empty-object-type -- the types match any operation of any document, which `any` and `{}` bounds say */

// ---- Types of the OpenAPI document -----------------------------------------------

export type HttpMethod = "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace";
export type MediaType = `${string}/${string}`;

type OkStatus = 200 | 201 | 202 | 203 | 204 | 206 | 207 | "2XX";
type ErrorStatus =
  | 500
  | 501
  | 502
  | 503
  | 504
  | 505
  | 506
  | 507
  | 508
  | 510
  | 511
  | "5XX"
  | 400
  | 401
  | 402
  | 403
  | 404
  | 405
  | 406
  | 407
  | 408
  | 409
  | 410
  | 411
  | 412
  | 413
  | 414
  | 415
  | 416
  | 417
  | 418
  | 420
  | 421
  | 422
  | 423
  | 424
  | 425
  | 426
  | 427
  | 428
  | 429
  | 430
  | 431
  | 444
  | 450
  | 451
  | 497
  | 498
  | 499
  | "4XX"
  | "default";

/** `Obj[K]` for the keys that match `Matchers` (a union of the matching members). */
export type FilterKeys<Obj, Matchers> = Obj[keyof Obj & Matchers];

/** The keys of `T` that can't be left out. */
export type RequiredKeysOf<T> = RequiredKeysOfHelper<T> extends undefined ? never : RequiredKeysOfHelper<T>;
type RequiredKeysOfHelper<T> = { [K in keyof T]: {} extends Pick<T, K> ? never : K }[keyof T];

/** The paths of `Paths` that have `Method`. */
export type PathsWithMethod<Paths extends {}, Method extends HttpMethod> = {
  [Path in keyof Paths]: Paths[Path] extends { [K in Method]: any } ? Path : never;
}[keyof Paths];

/** A response property the server sends and a request never carries (`readOnly`). */
export type $Read<T> = { readonly $read: T };
/** A request property the server never sends back (`writeOnly`). */
export type $Write<T> = { readonly $write: T };

/** `T` as a response has it: the write-only properties gone. */
export type Readable<T> =
  T extends $Write<any>
    ? never
    : T extends $Read<infer U>
      ? Readable<U>
      : T extends (infer E)[]
        ? Readable<E>[]
        : T extends object
          ? { [K in keyof T as NonNullable<T[K]> extends $Write<any> ? never : K]: Readable<T[K]> }
          : T;

/** `T` as a request has it: the read-only properties gone. */
export type Writable<T> =
  T extends $Read<any>
    ? never
    : T extends $Write<infer U>
      ? Writable<U>
      : T extends (infer E)[]
        ? Writable<E>[]
        : T extends object
          ? { [K in keyof T as NonNullable<T[K]> extends $Read<any> ? never : K]: Writable<T[K]> } & {
              [K in keyof T as NonNullable<T[K]> extends $Read<any> ? K : never]?: never;
            }
          : T;

type ResponseObjectMap<T> = T extends { responses: any } ? T["responses"] : unknown;
type ResponseContent<T> = T extends { content: any } ? T["content"] : unknown;
type OperationRequestBody<T> = "requestBody" extends keyof T ? T["requestBody"] : never;
type PickRequestBody<T> = "requestBody" extends keyof T ? Pick<T, "requestBody"> : never;
type IsOperationRequestBodyOptional<T> = RequiredKeysOf<PickRequestBody<T>> extends never ? true : false;
type OperationRequestBodyMediaContent<T> =
  IsOperationRequestBodyOptional<T> extends true
    ? ResponseContent<NonNullable<OperationRequestBody<T>>> | undefined
    : ResponseContent<OperationRequestBody<T>>;
/** The request body of an operation, in the media type it declares. */
export type OperationRequestBodyContent<T> =
  FilterKeys<OperationRequestBodyMediaContent<T>, MediaType> extends never
    ? FilterKeys<NonNullable<OperationRequestBodyMediaContent<T>>, MediaType> | undefined
    : FilterKeys<OperationRequestBodyMediaContent<T>, MediaType>;

type GetResponseContent<
  T extends Record<string | number, any>,
  Media extends MediaType = MediaType,
  Code extends keyof T = keyof T,
> = Code extends keyof T
  ? {
      [K in Code]: T[K]["content"] extends Record<string, any>
        ? FilterKeys<T[K]["content"], Media> extends never
          ? T[K]["content"]
          : FilterKeys<T[K]["content"], Media>
        : K extends keyof T
          ? T[K]["content"]
          : never;
    }[Code]
  : never;
/** The content of the success statuses of a `responses` map. */
export type SuccessResponse<
  T extends Record<string | number, any>,
  Media extends MediaType = MediaType,
> = GetResponseContent<T, Media, OkStatus>;
/** The content of the error statuses (4xx, 5xx, `default`) of a `responses` map. */
export type ErrorResponse<
  T extends Record<string | number, any>,
  Media extends MediaType = MediaType,
> = GetResponseContent<T, Media, ErrorStatus>;

// ---- Types of the client ---------------------------------------------------------

/** Options for each client. Everything but the ones named here is a `RequestInit` option sent with every request. */
export interface ClientOptions extends Omit<RequestInit, "headers"> {
  /** The root URL of the API; a trailing slash is dropped. Default `""`. */
  baseUrl?: string;
  /** The fetch to use. Default: `globalThis.fetch` when the client is created. */
  fetch?: (input: Request) => Promise<Response>;
  /** How query parameters are written: a function, or the OpenAPI styles. */
  querySerializer?: QuerySerializer<unknown> | QuerySerializerOptions;
  /** How a body is written. Default: JSON, a form for `application/x-www-form-urlencoded`, `FormData` as it is. */
  bodySerializer?: BodySerializer<unknown>;
  /** How path parameters are written. Default: {@link defaultPathSerializer}. */
  pathSerializer?: PathSerializer;
  /** Headers sent with every request. `null` removes a header. */
  headers?: HeadersOptions;
}

export type HeadersOptions =
  | Required<RequestInit>["headers"]
  | Record<string, string | number | boolean | (string | number | boolean)[] | null | undefined>;

export type QuerySerializer<T> = (
  query: T extends { parameters: any } ? NonNullable<T["parameters"]["query"]> : Record<string, unknown>,
) => string;

/** @see https://swagger.io/docs/specification/serialization/#query */
export interface QuerySerializerOptions {
  /** How arrays are written. Default `{ style: "form", explode: true }`: `a=1&a=2`. */
  array?: {
    style: "form" | "spaceDelimited" | "pipeDelimited";
    explode: boolean;
  };
  /** How objects are written. Default `{ style: "deepObject", explode: true }`: `o[k]=v`. */
  object?: {
    style: "form" | "deepObject";
    explode: boolean;
  };
  /** Leave the reserved characters `:/?#[]@!$&'()*+,;=` of values as they are. Default `false`. */
  allowReserved?: boolean;
}

export type BodySerializer<T> = (body: OperationRequestBodyContent<T>, headers: Headers) => any;

export type PathSerializer = (pathname: string, pathParams: Record<string, unknown>) => string;

type BodyType<T = unknown> = {
  json: T;
  text: Awaited<ReturnType<Response["text"]>>;
  blob: Awaited<ReturnType<Response["blob"]>>;
  arrayBuffer: Awaited<ReturnType<Response["arrayBuffer"]>>;
  stream: Response["body"];
};
/** How the body of a success is read: `json` (the default), `text`, `blob`, `arrayBuffer` or `stream`. */
export type ParseAs = keyof BodyType;
export type ParseAsResponse<T, Options> = Options extends { parseAs: ParseAs } ? BodyType<T>[Options["parseAs"]] : T;

export interface DefaultParamsOption {
  params?: { query?: Record<string, unknown> };
}

export type ParamsOption<T> = T extends { parameters: any }
  ? RequiredKeysOf<T["parameters"]> extends never
    ? { params?: T["parameters"] }
    : { params: T["parameters"] }
  : DefaultParamsOption;

// `Writable` takes the read-only properties out of what a request sends.
export type RequestBodyOption<T> =
  Writable<OperationRequestBodyContent<T>> extends never
    ? { body?: never }
    : IsOperationRequestBodyOptional<T> extends true
      ? { body?: Writable<OperationRequestBodyContent<T>> }
      : { body: Writable<OperationRequestBodyContent<T>> };

export type RequestOptions<T> = ParamsOption<T> &
  RequestBodyOption<T> & {
    baseUrl?: string;
    querySerializer?: QuerySerializer<T> | QuerySerializerOptions;
    bodySerializer?: BodySerializer<T>;
    pathSerializer?: PathSerializer;
    parseAs?: ParseAs;
    fetch?: ClientOptions["fetch"];
    headers?: HeadersOptions;
    middleware?: Middleware[];
  };

export type FetchOptions<T> = RequestOptions<T> & Omit<RequestInit, "body" | "headers">;

// `Readable` takes the write-only properties out of what a response carries.
export type FetchResponse<T extends Record<string | number, any>, Options, Media extends MediaType> =
  | {
      data: ParseAsResponse<Readable<SuccessResponse<ResponseObjectMap<T>, Media>>, Options>;
      error?: never;
      response: Response;
    }
  | {
      data?: never;
      error: Readable<ErrorResponse<ResponseObjectMap<T>, Media>>;
      response: Response;
    };

/** The client's options as middleware sees them, with every default filled in. */
export interface MergedOptions<T = unknown> {
  baseUrl: string;
  parseAs: ParseAs;
  querySerializer: QuerySerializer<T>;
  bodySerializer: BodySerializer<T>;
  pathSerializer: PathSerializer;
  fetch: typeof globalThis.fetch;
}

export interface MiddlewareRequestParams {
  query?: Record<string, unknown>;
  header?: Record<string, unknown>;
  path?: Record<string, unknown>;
  cookie?: Record<string, unknown>;
}

export interface MiddlewareCallbackParams {
  /** The request about to be sent. */
  request: Request;
  /** The path as the document writes it, braces included. */
  readonly schemaPath: string;
  /** The `params` of the call. */
  readonly params: MiddlewareRequestParams;
  /** One id for the request, the same in every hook. */
  readonly id: string;
  /** The options in force (read-only). */
  readonly options: MergedOptions;
}

type MiddlewareOnRequest = (
  options: MiddlewareCallbackParams,
) => void | Request | Response | undefined | Promise<Request | Response | undefined | void>;
type MiddlewareOnResponse = (
  options: MiddlewareCallbackParams & { response: Response },
) => void | Response | undefined | Promise<Response | undefined | void>;
type MiddlewareOnError = (
  options: MiddlewareCallbackParams & { error: unknown },
) => void | Response | Error | Promise<void | Response | Error>;

/**
 * Hooks around a request. `onRequest` may return a new `Request`, or a `Response` to answer
 * without calling fetch; `onResponse` may return a new `Response`; `onError` may return a
 * `Response` (the failure is handled) or an `Error` (a different failure).
 */
export type Middleware =
  | { onRequest: MiddlewareOnRequest; onResponse?: MiddlewareOnResponse; onError?: MiddlewareOnError }
  | { onRequest?: MiddlewareOnRequest; onResponse: MiddlewareOnResponse; onError?: MiddlewareOnError }
  | { onRequest?: MiddlewareOnRequest; onResponse?: MiddlewareOnResponse; onError: MiddlewareOnError };

/** The second parameter of a method: optional unless the operation needs params or a body. */
export type MaybeOptionalInit<Params, Location extends keyof Params> =
  RequiredKeysOf<FetchOptions<FilterKeys<Params, Location>>> extends never
    ? FetchOptions<FilterKeys<Params, Location>> | undefined
    : FetchOptions<FilterKeys<Params, Location>>;

// The inference has to finish before `[key: string]: unknown` is added, or TypeScript can't tell
// whether the parameter is required.
type InitParam<Init> =
  RequiredKeysOf<Init> extends never ? [(Init & { [key: string]: unknown })?] : [Init & { [key: string]: unknown }];

export type ClientMethod<
  Paths extends Record<string, Record<HttpMethod, {}>>,
  Method extends HttpMethod,
  Media extends MediaType,
> = <Path extends PathsWithMethod<Paths, Method>, Init extends MaybeOptionalInit<Paths[Path], Method>>(
  url: Path,
  ...init: InitParam<Init>
) => Promise<FetchResponse<Paths[Path][Method], Init, Media>>;

export type ClientRequestMethod<Paths extends Record<string, Record<HttpMethod, {}>>, Media extends MediaType> = <
  Method extends HttpMethod,
  Path extends PathsWithMethod<Paths, Method>,
  Init extends MaybeOptionalInit<Paths[Path], Method>,
>(
  method: Method,
  url: Path,
  ...init: InitParam<Init>
) => Promise<FetchResponse<Paths[Path][Method], Init, Media>>;

/** A typed client: one method per HTTP verb, taking a path of `Paths` that has it. */
export interface Client<Paths extends {}, Media extends MediaType = MediaType> {
  request: ClientRequestMethod<Paths, Media>;
  /** Call a GET route. */
  GET: ClientMethod<Paths, "get", Media>;
  /** Call a PUT route. */
  PUT: ClientMethod<Paths, "put", Media>;
  /** Call a POST route. */
  POST: ClientMethod<Paths, "post", Media>;
  /** Call a DELETE route. */
  DELETE: ClientMethod<Paths, "delete", Media>;
  /** Call an OPTIONS route. */
  OPTIONS: ClientMethod<Paths, "options", Media>;
  /** Call a HEAD route. */
  HEAD: ClientMethod<Paths, "head", Media>;
  /** Call a PATCH route. */
  PATCH: ClientMethod<Paths, "patch", Media>;
  /** Call a TRACE route. */
  TRACE: ClientMethod<Paths, "trace", Media>;
  /** Add middleware to every request the client makes. */
  use(...middleware: Middleware[]): void;
  /** Remove middleware added with `use`. */
  eject(...middleware: Middleware[]): void;
}

export type ClientPathsWithMethod<CreatedClient extends Client<any, any>, Method extends HttpMethod> =
  CreatedClient extends Client<infer Paths, infer _Media> ? PathsWithMethod<Paths, Method> : never;

/** The data type of a successful call: `MethodResponse<typeof api, "get", "/deals/{id}">`. */
export type MethodResponse<
  CreatedClient extends Client<any, any>,
  Method extends HttpMethod,
  Path extends ClientPathsWithMethod<CreatedClient, Method>,
  Options = {},
> =
  CreatedClient extends Client<infer Paths extends { [key: string]: any }, infer Media extends MediaType>
    ? NonNullable<FetchResponse<Paths[Path][Method], Options, Media>["data"]>
    : never;

// ---- Serialization ---------------------------------------------------------------

type ParamValue = string | number | boolean | null | undefined;

const encode = (value: unknown, allowReserved: boolean | undefined): string =>
  allowReserved === true ? String(value) : encodeURIComponent(value as string);

/** A name or an object key that came with the data: always percent-encoded, whatever `allowReserved` says. */
const encodeName = (name: string): string => encodeURIComponent(name);

/** `name=value` for one value; nothing for `null` and `undefined`. The name is written as it is given. */
export function serializePrimitiveParam(name: string, value: string, options?: { allowReserved?: boolean }): string {
  const v: unknown = value;
  if (v === undefined || v === null) return "";
  if (typeof v === "object") {
    throw new Error(
      "Deeply-nested arrays/objects aren’t supported. Provide your own `querySerializer()` to handle these.",
    );
  }
  return `${name}=${encode(v, options?.allowReserved)}`;
}

/** One level of an object, in one of the OpenAPI styles. */
export function serializeObjectParam(
  name: string,
  value: Record<string, unknown>,
  options: { style: "simple" | "label" | "matrix" | "form" | "deepObject"; explode: boolean; allowReserved?: boolean },
): string {
  if (!value || typeof value !== "object") return "";
  const joiner = ({ simple: ",", label: ".", matrix: ";" } as Record<string, string>)[options.style] ?? "&";

  if (options.style !== "deepObject" && options.explode === false) {
    const flat: string[] = [];
    for (const key in value) flat.push(encodeName(key), encode(value[key], options.allowReserved));
    const joined = flat.join(",");
    switch (options.style) {
      case "form":
        return `${name}=${joined}`;
      case "label":
        return `.${joined}`;
      case "matrix":
        return `;${name}=${joined}`;
      default:
        return joined;
    }
  }

  const parts: string[] = [];
  for (const key in value) {
    const part = serializePrimitiveParam(
      options.style === "deepObject" ? `${name}[${encodeName(key)}]` : encodeName(key),
      value[key] as string,
      options,
    );
    if (part) parts.push(part);
  }
  const joined = parts.join(joiner);
  return options.style === "label" || options.style === "matrix" ? `${joiner}${joined}` : joined;
}

/** One level of an array, in one of the OpenAPI styles. */
export function serializeArrayParam(
  name: string,
  value: unknown[],
  options: {
    style: "simple" | "label" | "matrix" | "form" | "spaceDelimited" | "pipeDelimited";
    explode: boolean;
    allowReserved?: boolean;
  },
): string {
  if (!Array.isArray(value)) return "";
  const items = value.filter((item) => item !== undefined && item !== null);

  if (options.explode === false) {
    const joiner =
      ({ form: ",", spaceDelimited: "%20", pipeDelimited: "|" } as Record<string, string>)[options.style] ?? ",";
    const joined = items.map((item) => encode(item, options.allowReserved)).join(joiner);
    switch (options.style) {
      case "simple":
        return joined;
      case "label":
        return `.${joined}`;
      case "matrix":
        return `;${name}=${joined}`;
      default:
        return `${name}=${joined}`;
    }
  }

  const joiner = ({ simple: ",", label: ".", matrix: ";" } as Record<string, string>)[options.style] ?? "&";
  const parts = items.map((item) =>
    options.style === "simple" || options.style === "label"
      ? encode(item, options.allowReserved)
      : serializePrimitiveParam(name, item as string, options),
  );
  const joined = parts.join(joiner);
  return options.style === "label" || options.style === "matrix" ? `${joiner}${joined}` : joined;
}

/** A query serializer for the OpenAPI styles: arrays as `a=1&a=2`, objects as `o[k]=v`, by default. */
export function createQuerySerializer<T = unknown>(options?: QuerySerializerOptions): (queryParams: T) => string {
  return (queryParams) => {
    const search: string[] = [];
    if (queryParams && typeof queryParams === "object") {
      const params = queryParams as Record<string, unknown>;
      for (const key in params) {
        const value = params[key] as ParamValue | object;
        if (value === undefined || value === null) continue;
        const name = encodeName(key);
        let part: string;
        if (Array.isArray(value)) {
          if (value.length === 0) continue;
          part = serializeArrayParam(name, value, {
            style: "form",
            explode: true,
            ...options?.array,
            allowReserved: options?.allowReserved || false,
          });
        } else if (typeof value === "object") {
          part = serializeObjectParam(name, value as Record<string, unknown>, {
            style: "deepObject",
            explode: true,
            ...options?.object,
            allowReserved: options?.allowReserved || false,
          });
        } else {
          part = serializePrimitiveParam(name, value as string, options);
        }
        if (part) search.push(part);
      }
    }
    return search.join("&");
  };
}

const PLACEHOLDER = /\{[^{}]+\}/g;

/** A segment a URL reads as the one before it or the same one: `.`, `..` and their `%2e` spellings. */
const DOT_SEGMENT = /^(\.|%2e){1,2}(;.*)?$/i;

/**
 * Fills the `{name}` placeholders of a path from `pathParams`, in the OpenAPI path styles:
 * `{id}` simple, `{.id}` label, `{;id}` matrix, and a trailing `*` to explode a list or an object.
 * A placeholder without a value stays as it is. Values and object keys are percent-encoded, and a
 * value that leaves `.` or `..` as a whole segment is refused, as a URL would read it as a step
 * up or no step at all.
 */
export function defaultPathSerializer(pathname: string, pathParams: Record<string, unknown>): string {
  let url = "";
  let copied = 0;
  const filled: { name: string; start: number; end: number }[] = [];
  for (const found of pathname.matchAll(PLACEHOLDER)) {
    const match = found[0];
    let name = match.slice(1, -1);
    let explode = false;
    let style: "simple" | "label" | "matrix" = "simple";
    if (name.endsWith("*")) {
      explode = true;
      name = name.slice(0, -1);
    }
    if (name.startsWith(".")) {
      style = "label";
      name = name.slice(1);
    } else if (name.startsWith(";")) {
      style = "matrix";
      name = name.slice(1);
    }
    const value = pathParams?.[name];
    if (value === undefined || value === null) continue;
    let text: string;
    if (Array.isArray(value)) {
      text = serializeArrayParam(name, value, { style, explode });
    } else if (typeof value === "object") {
      text = serializeObjectParam(name, value as Record<string, unknown>, { style, explode });
    } else if (style === "matrix") {
      text = `;${serializePrimitiveParam(name, value as string)}`;
    } else {
      text = style === "label" ? `.${encodeURIComponent(value as string)}` : encodeURIComponent(value as string);
    }
    url += pathname.slice(copied, found.index);
    filled.push({ name, start: url.length, end: url.length + text.length });
    url += text;
    copied = found.index + match.length;
  }
  url += pathname.slice(copied);
  for (const { name, start, end } of filled) {
    const from = start === 0 ? 0 : url.lastIndexOf("/", start - 1) + 1;
    let to = end;
    while (to < url.length && url[to] !== "/" && url[to] !== "?" && url[to] !== "#") to++;
    if (DOT_SEGMENT.test(url.slice(from, to))) {
      throw new Error(`The path parameter "${name}" would make "." or ".." a segment of the path (a dot segment).`);
    }
  }
  return url;
}

/** The body as sent: `FormData` as it is, a form for `application/x-www-form-urlencoded`, JSON otherwise. */
export function defaultBodySerializer(body: unknown, headers?: Headers | Record<string, string | undefined>): BodyInit {
  if (typeof FormData !== "undefined" && body instanceof FormData) return body;
  if (headers) {
    const contentType =
      headers instanceof Headers ? headers.get("content-type") : (headers["Content-Type"] ?? headers["content-type"]);
    if (contentType === "application/x-www-form-urlencoded") {
      return new URLSearchParams(body as Record<string, string>).toString();
    }
  }
  return JSON.stringify(body);
}

/** The URL of a call: base, path with its parameters, and the serialized query. */
export function createFinalURL<O>(
  pathname: string,
  options: {
    baseUrl: string;
    params: { query?: Record<string, unknown>; path?: Record<string, unknown> };
    querySerializer: QuerySerializer<O>;
    pathSerializer: PathSerializer;
  },
): string {
  const template = `${options.baseUrl}${pathname}`;
  let url = template;
  if (options.params?.path) url = options.pathSerializer(url, options.params.path);
  // An empty first segment makes `//host/...`, which a URL reads as another host.
  if (url.startsWith("//") && !template.startsWith("//")) {
    throw new Error("The path parameters make the URL start with `//`, which names another host.");
  }
  let search = (options.querySerializer as (query: unknown) => string)(options.params.query ?? {});
  if (search.startsWith("?")) search = search.slice(1);
  return search ? `${url}?${search}` : url;
}

/** One `Headers` from several sets, later ones winning; `null` removes a header, `undefined` is skipped. */
export function mergeHeaders(...allHeaders: (HeadersOptions | undefined)[]): Headers {
  const merged = new Headers();
  for (const headers of allHeaders) {
    if (!headers || typeof headers !== "object") continue;
    const entries: Iterable<[string, unknown]> =
      headers instanceof Headers ? headers.entries() : Object.entries(headers as Record<string, unknown>);
    for (const [name, value] of entries) {
      if (value === null) merged.delete(name);
      else if (Array.isArray(value)) for (const item of value) merged.append(name, item);
      else if (value !== undefined) merged.set(name, value as string);
    }
  }
  return merged;
}

const withoutTrailingSlash = (url: string): string => (url.endsWith("/") ? url.slice(0, -1) : url);

// ---- The client ------------------------------------------------------------------

/** A cheap id for correlating the hooks of one request; not for security. */
const requestId = (): string => Math.random().toString(36).slice(2, 11);

/**
 * Creates a client for the routes of `Paths`. Every call resolves to `{ data, response }` for a
 * success or `{ error, response }` for an error status, and rejects only when the request fails
 * (no connection, an abort, a hook that throws).
 */
export default function createClient<Paths extends {}, Media extends MediaType = MediaType>(
  clientOptions?: ClientOptions,
): Client<Paths, Media> {
  const {
    baseUrl: clientBaseUrl = "",
    fetch: clientFetch = globalThis.fetch,
    querySerializer: clientQuerySerializer,
    bodySerializer: clientBodySerializer,
    pathSerializer: clientPathSerializer,
    headers: clientHeaders,
    ...clientInit
  } = { ...clientOptions };
  const baseUrl = withoutTrailingSlash(clientBaseUrl);
  const clientMiddleware: Middleware[] = [];

  async function coreFetch(schemaPath: string, fetchOptions?: Record<string, any>) {
    const {
      baseUrl: requestBaseUrl,
      fetch = clientFetch,
      headers,
      params = {},
      parseAs = "json",
      querySerializer: requestQuerySerializer,
      bodySerializer = clientBodySerializer ?? defaultBodySerializer,
      pathSerializer: requestPathSerializer,
      body,
      middleware: requestMiddleware = [],
      ...init
    } = fetchOptions ?? {};
    const finalBaseUrl = requestBaseUrl ? withoutTrailingSlash(requestBaseUrl) : baseUrl;

    // A request's function replaces the client's; a request's options merge over the client's.
    let querySerializer: (query: any) => string =
      typeof clientQuerySerializer === "function"
        ? clientQuerySerializer
        : createQuerySerializer(clientQuerySerializer);
    if (requestQuerySerializer) {
      querySerializer =
        typeof requestQuerySerializer === "function"
          ? requestQuerySerializer
          : createQuerySerializer({
              ...(typeof clientQuerySerializer === "object" ? clientQuerySerializer : {}),
              ...requestQuerySerializer,
            });
    }
    const pathSerializer: PathSerializer = requestPathSerializer ?? clientPathSerializer ?? defaultPathSerializer;

    // The serializer sees every header the caller set (the form content type is one of them);
    // the content type it implies is the lowest priority, so it is merged below in front of them.
    const serialized =
      body === undefined ? undefined : bodySerializer(body, mergeHeaders(clientHeaders, headers, params.header));
    const keepsOwnContentType =
      serialized === undefined || (typeof FormData !== "undefined" && serialized instanceof FormData);
    const finalHeaders = mergeHeaders(
      keepsOwnContentType ? {} : { "Content-Type": "application/json" },
      clientHeaders,
      headers,
      params.header,
    );

    const middleware = [...clientMiddleware, ...requestMiddleware];
    const requestInit = {
      redirect: "follow",
      ...clientInit,
      ...init,
      body: serialized,
      headers: finalHeaders,
    } as RequestInit;
    let request = new Request(
      createFinalURL(schemaPath, { baseUrl: finalBaseUrl, params, querySerializer, pathSerializer }),
      requestInit,
    );
    // Keys `Request` doesn't know are kept on it, for middleware to read.
    for (const key in init) {
      if (!(key in request)) (request as unknown as Record<string, unknown>)[key] = init[key];
    }

    let response: Response | undefined;
    let id = "";
    let options: Readonly<MergedOptions> | undefined;
    const context = () => ({ request, schemaPath, params, options: options as MergedOptions, id });

    if (middleware.length) {
      id = requestId();
      options = Object.freeze({
        baseUrl: finalBaseUrl,
        fetch,
        parseAs,
        querySerializer,
        bodySerializer,
        pathSerializer,
      }) as Readonly<MergedOptions>;
      for (const hook of middleware) {
        if (!hook || typeof hook.onRequest !== "function") continue;
        const result = await hook.onRequest(context());
        if (!result) continue;
        if (result instanceof Request) request = result;
        else if (result instanceof Response) {
          response = result;
          break;
        } else throw new Error("onRequest: must return new Request() or Response() when modifying the request");
      }
    }

    if (!response) {
      try {
        response = await fetch(request);
      } catch (error) {
        let failure: unknown = error;
        // The last middleware sees the failure first.
        for (let i = middleware.length - 1; i >= 0; i--) {
          const hook = middleware[i];
          if (!hook || typeof hook.onError !== "function") continue;
          const result = await hook.onError({ ...context(), error: failure });
          if (!result) continue;
          if (result instanceof Response) {
            failure = undefined;
            response = result;
            break;
          }
          if (result instanceof Error) {
            failure = result;
            continue;
          }
          throw new Error("onError: must return new Response() or instance of Error");
        }
        if (failure) throw failure;
      }

      for (let i = middleware.length - 1; i >= 0; i--) {
        const hook = middleware[i];
        if (!hook || typeof hook.onResponse !== "function") continue;
        const result = await hook.onResponse({ ...context(), response: response as Response });
        if (!result) continue;
        if (!(result instanceof Response))
          throw new Error("onResponse: must return new Response() when modifying the response");
        response = result;
      }
    }

    const answer = response as Response;
    const contentLength = answer.headers.get("Content-Length");
    const empty =
      answer.status === 204 ||
      request.method === "HEAD" ||
      (contentLength === "0" && !answer.headers.get("Transfer-Encoding")?.includes("chunked"));
    if (empty) return answer.ok ? { data: undefined, response: answer } : { error: undefined, response: answer };

    if (answer.ok) {
      let data: unknown;
      if (parseAs === "stream") data = answer.body;
      else if (parseAs === "json" && !contentLength) {
        // Without a length the body may be empty, which `json()` would refuse.
        const text = await answer.text();
        data = text ? JSON.parse(text) : undefined;
      } else data = await answer[parseAs as "json"]();
      return { data, response: answer };
    }

    // An error body is JSON when it parses, its text otherwise.
    let error: unknown = await answer.text();
    try {
      error = JSON.parse(error as string);
    } catch {
      // The text stays.
    }
    return { error, response: answer };
  }

  const call = (method: string) => (url: string, init?: Record<string, unknown>) => coreFetch(url, { ...init, method });

  const client = {
    request: (method: string, url: string, init?: Record<string, unknown>) =>
      coreFetch(url, { ...init, method: method.toUpperCase() }),
    GET: call("GET"),
    PUT: call("PUT"),
    POST: call("POST"),
    DELETE: call("DELETE"),
    OPTIONS: call("OPTIONS"),
    HEAD: call("HEAD"),
    PATCH: call("PATCH"),
    TRACE: call("TRACE"),
    use(...middleware: Middleware[]) {
      for (const hook of middleware) {
        if (!hook) continue;
        if (typeof hook !== "object" || !("onRequest" in hook || "onResponse" in hook || "onError" in hook)) {
          throw new Error("Middleware must be an object with one of `onRequest()`, `onResponse() or `onError()`");
        }
        clientMiddleware.push(hook);
      }
    },
    eject(...middleware: Middleware[]) {
      for (const hook of middleware) {
        const index = clientMiddleware.indexOf(hook);
        if (index !== -1) clientMiddleware.splice(index, 1);
      }
    },
  };
  return client as unknown as Client<Paths, Media>;
}

export { createClient };
