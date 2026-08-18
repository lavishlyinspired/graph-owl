import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Testing Library only auto-cleans when vitest runs with `globals: true`,
// which this project does not. Without this, each test's render stacks into
// the same document and queries start matching the previous test's output.
afterEach(cleanup);
