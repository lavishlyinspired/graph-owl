/** One form renderer for every connector — Epic 41 Slice F.
 *
 *  A hundred connectors with hand-written forms is a hundred places for a field
 *  to go missing, and the one that goes missing is always the optional-looking
 *  one somebody actually needed. The connector declares its shape as JSON
 *  Schema; this turns that into fields.
 *
 *  `00f-ui-architecture.md` puts the part that can be wrong in a way somebody
 *  would act on in a pure function. Here that is: which fields exist, which are
 *  required, which are secret, and **which cannot be rendered at all** — because
 *  a field this build does not understand must be visible as unsupported rather
 *  than quietly drawn as a text box that submits the wrong type.
 */

/** What a field is, in the terms a renderer needs. */
export type FieldKind = "text" | "number" | "boolean" | "secret" | "list" | "unsupported";

export interface Field {
  readonly name: string;
  /** `title` when the schema gives one, else the property name. A schema author
   *  who wrote a title meant it to be shown. */
  readonly label: string;
  readonly kind: FieldKind;
  readonly required: boolean;
  readonly description?: string;
  /** Only for `number`/`text`/`boolean`. **Never for `secret`** — see below. */
  readonly initial?: string | number | boolean;
}

/** A schema property, as much of it as this renderer reads. */
interface Property {
  readonly type?: string;
  readonly title?: string;
  readonly description?: string;
  readonly default?: unknown;
  /** JSON Schema's own vocabulary for "may be written, never read back". */
  readonly writeOnly?: boolean;
}

export interface Schema {
  readonly properties?: Readonly<Record<string, Property>>;
  readonly required?: readonly string[];
}

/** Which control a property needs.
 *
 *  **An unrecognised type is `unsupported`, never `text`.** A text input for an
 *  object or a nested array submits a string where the connector expects
 *  structure, and the failure arrives at connection time as a type error nobody
 *  can trace back to the form. Saying "this build cannot render this field" is
 *  a worse-looking screen and a better outcome.
 */
export function kindOf(property: Property): FieldKind {
  // Checked before `type`, because a secret is a string and would otherwise
  // render as an ordinary text input with the value on screen.
  if (property.writeOnly === true) return "secret";
  switch (property.type) {
    case "string":
      return "text";
    case "integer":
    case "number":
      return "number";
    case "boolean":
      return "boolean";
    case "array":
      return "list";
    default:
      return "unsupported";
  }
}

/** The fields a schema declares, in the order it declares them.
 *
 *  Schema order, not alphabetical: an author who put `host` before `port` meant
 *  that reading order, and sorting would rewrite the shape of every form.
 *
 *  A schema with no `properties` yields no fields rather than throwing — a
 *  connector that needs no configuration is a legitimate connector, and a form
 *  that crashed on one would take the whole admin page with it.
 */
export function fields(schema: Schema, values: Readonly<Record<string, unknown>> = {}): Field[] {
  return Object.entries(schema.properties ?? {}).map(([name, property]) => {
    const kind = kindOf(property);
    const field: Field = {
      name,
      label: property.title ?? name,
      kind,
      // Asked of the schema directly. A `Set` built from `required ?? []` was
      // an allocation and a second defaulting step for no gain — and a schema
      // with no `required` key requires nothing, which `?.` says more plainly.
      required: schema.required?.includes(name) ?? false,
      ...(property.description === undefined ? {} : { description: property.description }),
      // **A secret is never given an initial value**, whatever was passed in.
      // There is nothing legitimate to populate it from — `ConnectorConfig`
      // carries no credential — so a value reaching here for a `writeOnly`
      // field came from somewhere it should not have, and rendering it would
      // put a password on screen.
      ...(kind === "secret" ? {} : initialFor(property, values[name])),
    };
    return field;
  });
}

function initialFor(
  property: Property,
  value: unknown,
): { initial: string | number | boolean } | Record<string, never> {
  const candidate = value ?? property.default;
  if (typeof candidate === "string" || typeof candidate === "number" || typeof candidate === "boolean") {
    return { initial: candidate };
  }
  // An unusable default is omitted rather than coerced. `String({})` is
  // `"[object Object]"`, which a form would happily submit.
  return {};
}

/** Which required fields a submission is missing.
 *
 *  **A required secret is only missing when nothing is stored.** An edit form
 *  cannot resend a credential it was never given, so demanding one on every
 *  save would make changing a port impossible without retyping the password —
 *  and the person who does not have it to hand will clear the field instead.
 */
export function missing(
  schema: Schema,
  submitted: Readonly<Record<string, unknown>>,
  hasStoredSecret = false,
): string[] {
  return fields(schema)
    .filter((field) => field.required)
    .filter((field) => {
      if (field.kind === "secret" && hasStoredSecret) return false;
      const value = submitted[field.name];
      // An empty string is not a value. It is what a required field looks like
      // after somebody has clicked into it and out again.
      return value === undefined || value === null || value === "";
    })
    .map((field) => field.name);
}

/** Can this schema be rendered at all?
 *
 *  A form with an unsupported *required* field cannot be completed, so offering
 *  it is worse than refusing: the operator fills in six fields and the save
 *  fails on the seventh they could not see. An unsupported *optional* field is
 *  a gap worth showing but not a blocker.
 */
export function renderable(schema: Schema): { ok: boolean; blocking: string[] } {
  const blocking = fields(schema)
    .filter((field) => field.kind === "unsupported" && field.required)
    .map((field) => field.name);
  return { ok: blocking.length === 0, blocking };
}
