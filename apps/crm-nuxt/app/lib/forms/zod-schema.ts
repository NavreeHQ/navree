import type { TypedSchema, TypedSchemaError } from "vee-validate";
import type { z } from "zod";

/**
 * Adapts a zod 4 schema to VeeValidate's `TypedSchema`, so `useForm` validates
 * with zod and types `handleSubmit` values with the schema's output.
 *
 * Issues become field errors keyed by their dotted path (`company.id`), which is
 * how VeeValidate names nested fields; refinements on the whole object
 * (`superRefine` with an explicit `path`) land on that field.
 */
export function toTypedSchema<S extends z.ZodType>(schema: S): TypedSchema<z.input<S>, z.output<S>> {
  return {
    __type: "VVTypedSchema",
    async parse(values) {
      const result = await schema.safeParseAsync(values);
      if (result.success) return { value: result.data, errors: [] };
      const byPath = new Map<string, string[]>();
      for (const issue of result.error.issues) {
        const path = issue.path.map(String).join(".");
        const list = byPath.get(path) ?? [];
        list.push(issue.message);
        byPath.set(path, list);
      }
      const errors: TypedSchemaError[] = [...byPath].map(([path, messages]) => ({ path, errors: messages }));
      return { errors };
    },
  };
}
