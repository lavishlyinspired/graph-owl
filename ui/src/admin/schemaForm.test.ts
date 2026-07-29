import { describe, expect, it } from "vitest";
import { type Schema, fields, kindOf, missing, renderable } from "./schemaForm";

/** The Postgres connector's own schema, near enough. */
const postgres: Schema = {
  properties: {
    host: { type: "string", title: "Host" },
    port: { type: "integer", title: "Port", default: 5432 },
    database: { type: "string", title: "Database" },
    password: { type: "string", title: "Password", writeOnly: true },
    includeSchemas: { type: "array", title: "Schemas", description: "Empty means all." },
  },
  required: ["host", "database", "password"],
};

describe("which control a property needs", () => {
  it("maps each type it understands", () => {
    expect(kindOf({ type: "string" })).toBe("text");
    expect(kindOf({ type: "integer" })).toBe("number");
    expect(kindOf({ type: "number" })).toBe("number");
    expect(kindOf({ type: "boolean" })).toBe("boolean");
    expect(kindOf({ type: "array" })).toBe("list");
  });

  // **A secret is a string**, so it has to be checked before `type` or it
  // renders as an ordinary text input with the value on screen.
  it("recognises a secret before it recognises a string", () => {
    expect(kindOf({ type: "string", writeOnly: true })).toBe("secret");
  });

  // A text input for an object submits a string where the connector expects
  // structure, and the failure arrives at connection time as a type error
  // nobody can trace back to the form.
  it("refuses to guess at a type it does not know", () => {
    expect(kindOf({ type: "object" })).toBe("unsupported");
    expect(kindOf({})).toBe("unsupported");
    expect(kindOf({ type: "null" })).toBe("unsupported");
  });
});

describe("the fields a schema declares", () => {
  // An author who put `host` before `port` meant that reading order. Sorting
  // would rewrite the shape of every form.
  it("keeps schema order rather than sorting", () => {
    expect(fields(postgres).map((f) => f.name)).toEqual([
      "host",
      "port",
      "database",
      "password",
      "includeSchemas",
    ]);
  });

  it("uses the title when the schema gives one", () => {
    expect(fields(postgres)[0]!.label).toBe("Host");
  });

  // A schema author who wrote no title did not mean the field to be nameless.
  it("falls back to the property name", () => {
    expect(fields({ properties: { retries: { type: "integer" } } })[0]!.label).toBe("retries");
  });

  // A schema with no `required` array requires nothing. Defaulting to anything
  // else would mark every field mandatory on a connector whose author simply
  // did not write the key.
  it("requires nothing when the schema says nothing", () => {
    const schema: Schema = { properties: { host: { type: "string" }, port: { type: "integer" } } };

    expect(fields(schema).filter((f) => f.required)).toEqual([]);
    expect(missing(schema, {})).toEqual([]);
  });

  it("marks the required ones and only those", () => {
    const required = fields(postgres).filter((f) => f.required).map((f) => f.name);

    expect(required).toEqual(["host", "database", "password"]);
  });

  it("carries a description through when there is one", () => {
    const list = fields(postgres).find((f) => f.name === "includeSchemas");

    expect(list?.description).toBe("Empty means all.");
    // The **key is absent**, not present-and-undefined. A field carrying
    // `description: undefined` serialises differently and reads as "this field
    // has an empty description" to anything iterating keys.
    expect("description" in fields(postgres)[0]!).toBe(false);
  });

  // A connector that needs no configuration is legitimate, and a form that
  // crashed on one would take the whole admin page with it.
  it("yields nothing for a schema with no properties", () => {
    expect(fields({})).toEqual([]);
    expect(fields({ properties: {} })).toEqual([]);
  });
});

describe("initial values", () => {
  it("uses the schema default", () => {
    expect(fields(postgres).find((f) => f.name === "port")?.initial).toBe(5432);
  });

  it("prefers a supplied value over the default", () => {
    expect(fields(postgres, { port: 6543 }).find((f) => f.name === "port")?.initial).toBe(6543);
  });

  // Strings are the common case — a host, a database name — and a check that
  // only admitted numbers and booleans would silently drop every one of them.
  it("populates a string value", () => {
    expect(fields(postgres, { host: "db.internal" })[0]!.initial).toBe("db.internal");
  });

  it("populates a boolean value", () => {
    const schema: Schema = { properties: { ssl: { type: "boolean" } } };

    expect(fields(schema, { ssl: false })[0]!.initial).toBe(false);
  });

  // **The one that matters.** There is nothing legitimate to populate a secret
  // from — `ConnectorConfig` carries no credential — so a value reaching here
  // for a `writeOnly` field came from somewhere it should not have, and
  // rendering it would put a password on screen.
  it("never populates a secret, even when handed one", () => {
    const password = fields(postgres, { password: "s3cr3t" }).find((f) => f.name === "password");

    expect(password?.kind).toBe("secret");
    expect(password?.initial).toBeUndefined();
    expect(JSON.stringify(password)).not.toContain("s3cr3t");
  });

  // And a secret with a default in the schema is still not populated — a schema
  // author writing a default password is a mistake this must not amplify.
  it("ignores a default on a secret too", () => {
    const schema: Schema = {
      properties: { token: { type: "string", writeOnly: true, default: "changeme" } },
    };

    expect(fields(schema)[0]!.initial).toBeUndefined();
  });

  // `String({})` is `"[object Object]"`, which a form would happily submit.
  it("omits a default it cannot render rather than coercing it", () => {
    const schema: Schema = { properties: { opts: { type: "string", default: { a: 1 } } } };

    expect(fields(schema)[0]!.initial).toBeUndefined();
  });
});

describe("what a submission is missing", () => {
  it("names the required fields that are absent", () => {
    expect(missing(postgres, { host: "db" })).toEqual(["database", "password"]);
  });

  // An empty string is what a required field looks like after somebody has
  // clicked into it and out again.
  it("treats an empty string as absent", () => {
    expect(missing(postgres, { host: "", database: "d", password: "p" })).toEqual(["host"]);
  });

  // Each way a field can be blank, separately: an untouched field is
  // `undefined`, a cleared one often serialises to `null`, and one clicked into
  // and out of is `""`. A check missing any of the three lets a save through
  // that fails at connection time.
  it.each([
    ["undefined", undefined],
    ["null", null],
    ["empty string", ""],
  ])("treats a %s value as absent", (_label, blank) => {
    expect(missing(postgres, { host: blank, database: "d", password: "p" })).toEqual(["host"]);
  });

  it("says nothing when everything required is present", () => {
    expect(missing(postgres, { host: "db", database: "d", password: "p" })).toEqual([]);
  });

  // **A required secret is only missing when nothing is stored.** Demanding one
  // on every save would make changing a port impossible without retyping the
  // password — and somebody who does not have it to hand will clear the field.
  it("does not demand a secret that is already stored", () => {
    expect(missing(postgres, { host: "db", database: "d" }, true)).toEqual([]);
  });

  // **A stored secret excuses the secret, not everything else.** A check that
  // skipped every field once a credential existed would let a save through with
  // no database name, and the failure would arrive at connection time.
  it("still reports other missing fields when a secret is stored", () => {
    expect(missing(postgres, { host: "db" }, true)).toEqual(["database"]);
  });

  it("still demands one when none is stored", () => {
    expect(missing(postgres, { host: "db", database: "d" }, false)).toEqual(["password"]);
  });

  it("ignores optional fields entirely", () => {
    expect(missing(postgres, { host: "db", database: "d", password: "p" })).not.toContain("port");
  });
});

describe("whether a schema can be rendered", () => {
  // An operator fills in six fields and the save fails on a seventh they could
  // not see. Refusing the form is the better outcome.
  it("refuses a schema whose required field cannot be rendered", () => {
    const schema: Schema = {
      properties: { creds: { type: "object", title: "Credentials" } },
      required: ["creds"],
    };

    expect(renderable(schema)).toEqual({ ok: false, blocking: ["creds"] });
  });

  // An unsupported *optional* field is a gap worth showing, not a blocker.
  it("allows a schema whose unsupported field is optional", () => {
    const schema: Schema = { properties: { extra: { type: "object" } }, required: [] };

    expect(renderable(schema).ok).toBe(true);
  });

  it("allows a schema it understands entirely", () => {
    expect(renderable(postgres)).toEqual({ ok: true, blocking: [] });
  });
});
