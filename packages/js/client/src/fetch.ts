export {
  default as createClient,
  createFinalURL,
  createQuerySerializer,
  defaultBodySerializer,
  defaultPathSerializer,
  mergeHeaders,
  serializeArrayParam,
  serializeObjectParam,
  serializePrimitiveParam,
} from "openapi-fetch";
export type { Client, Middleware } from "openapi-fetch";
