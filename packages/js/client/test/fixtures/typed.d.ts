/**
 * A hand-written `paths` type in the shape the schema generator writes, with one
 * operation for each thing the typed fetch core has to infer: required and
 * optional path, query and header parameters, bodies that are required, optional
 * or in several media types, responses by status and by content type, ranges,
 * `default`, empty responses, and read/write markers.
 */
export interface Pet {
  id: string;
  name: string;
  tag?: string;
}

export interface NewPet {
  name: string;
  tag?: string;
}

export interface Problem {
  code: string;
  message: string;
}

/** The markers the schema generator writes for readOnly and writeOnly properties. */
type $Read<T> = { readonly $read: T };
type $Write<T> = { readonly $write: T };

export interface Account {
  id: $Read<string>;
  email: string;
  password: $Write<string>;
}

export interface paths {
  "/pets": {
    parameters: { query?: never; header?: never; path?: never; cookie?: never };
    get: {
      parameters: {
        query?: { limit?: number; tags?: string[]; filter?: { name?: string; tag?: string }; q?: string | null };
        header?: never;
        path?: never;
        cookie?: never;
      };
      requestBody?: never;
      responses: {
        200: { headers: { [name: string]: unknown }; content: { "application/json": Pet[] } };
        default: { headers: { [name: string]: unknown }; content: { "application/json": Problem } };
      };
    };
    put?: never;
    post: {
      parameters: { query?: never; header?: never; path?: never; cookie?: never };
      requestBody: {
        content: {
          "application/json": NewPet;
          "application/x-www-form-urlencoded": { name: string; tag?: string };
        };
      };
      responses: {
        201: { headers: { [name: string]: unknown }; content: { "application/json": Pet } };
        400: { headers: { [name: string]: unknown }; content: { "application/json": Problem } };
        422: { headers: { [name: string]: unknown }; content: { "application/json": { fields: Record<string, string[]> } } };
      };
    };
    delete?: never;
    options?: never;
    head?: never;
    patch?: never;
    trace?: never;
  };
  "/pets/{petId}": {
    parameters: { query?: never; header?: never; path?: never; cookie?: never };
    get: {
      parameters: {
        query?: never;
        header?: { "X-Trace"?: string };
        path: { petId: string };
        cookie?: never;
      };
      requestBody?: never;
      responses: {
        200: { headers: { [name: string]: unknown }; content: { "application/json": Pet } };
        404: { headers: { [name: string]: unknown }; content: { "application/json": Problem } };
        500: { headers: { [name: string]: unknown }; content: { "application/json": { trace: string } } };
      };
    };
    put?: never;
    post?: never;
    delete: {
      parameters: { query?: never; header?: never; path: { petId: string }; cookie?: never };
      requestBody?: never;
      responses: {
        204: { headers: { [name: string]: unknown }; content?: never };
        404: { headers: { [name: string]: unknown }; content: { "application/json": Problem } };
      };
    };
    options?: never;
    head?: never;
    patch: {
      parameters: { query?: never; header?: never; path: { petId: string }; cookie?: never };
      requestBody?: { content: { "application/json": Partial<NewPet> } };
      responses: {
        200: { headers: { [name: string]: unknown }; content: { "application/json": Pet } };
      };
    };
    trace?: never;
  };
  "/search": {
    parameters: { query?: never; header?: never; path?: never; cookie?: never };
    get: {
      parameters: { query: { q: string; page?: number }; header?: never; path?: never; cookie?: never };
      requestBody?: never;
      responses: {
        200: { headers: { [name: string]: unknown }; content: { "application/json": { hits: Pet[] } } };
      };
    };
    put?: never;
    post?: never;
    delete?: never;
    options?: never;
    head?: never;
    patch?: never;
    trace?: never;
  };
  "/files/{name}": {
    parameters: { query?: never; header?: never; path?: never; cookie?: never };
    get: {
      parameters: { query?: never; header?: never; path: { name: string }; cookie?: never };
      requestBody?: never;
      responses: {
        200: {
          headers: { [name: string]: unknown };
          content: { "application/octet-stream": string; "application/json": { name: string; size: number } };
        };
        "4XX": { headers: { [name: string]: unknown }; content: { "application/json": Problem } };
      };
    };
    put: {
      parameters: { query?: never; header?: never; path: { name: string }; cookie?: never };
      requestBody: { content: { "multipart/form-data": { file: string } } };
      responses: {
        "2XX": { headers: { [name: string]: unknown }; content: { "application/json": { stored: true } } };
      };
    };
    post?: never;
    delete?: never;
    options?: never;
    head: {
      parameters: { query?: never; header?: never; path: { name: string }; cookie?: never };
      requestBody?: never;
      responses: { 200: { headers: { [name: string]: unknown }; content?: never } };
    };
    patch?: never;
    trace?: never;
  };
  "/accounts": {
    parameters: { query?: never; header?: never; path?: never; cookie?: never };
    get?: never;
    put?: never;
    post: {
      parameters: { query?: never; header?: never; path?: never; cookie?: never };
      requestBody: {
        content: { "application/json": Account };
      };
      responses: {
        201: { headers: { [name: string]: unknown }; content: { "application/json": Account } };
      };
    };
    delete?: never;
    options?: never;
    head?: never;
    patch?: never;
    trace?: never;
  };
}
