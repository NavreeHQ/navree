/** Server middleware: pages behind sign-in, and expired tokens refreshed before a page renders. */
import { defineEventHandler, type EventHandler } from "h3";
import { useRuntimeConfig } from "#imports";
import { createSessionGate } from "../../server/handlers.js";
import { useNavreeBff } from "./utils.js";

let gate: EventHandler | undefined;

export default defineEventHandler((event) => {
  if (!gate) {
    const options = useRuntimeConfig(event).navree.sessionGate;
    if (options === false) return;
    gate = createSessionGate(useNavreeBff, options);
  }
  return gate(event);
});
