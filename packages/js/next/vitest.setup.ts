import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Testing Library unmounts after each test only when vitest's globals are on; they are off here.
afterEach(() => cleanup());
