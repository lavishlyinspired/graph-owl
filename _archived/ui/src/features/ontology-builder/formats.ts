/** Import/export for ontology models in RDF-ish serialisations.
 *
 *  The formats supported are the ones most commonly asked for when moving
 *  an ontology in or out of a metadata platform: Turtle, JSON-LD, N-Triples,
 *  and OWL/RDF XML. The parsers are intentionally small — they handle the
 *  subset this builder produces, plus hand-written ontologies that follow
 *  the same simple shape. They are not general-purpose RDF processors. */

import type { Cardinality, OntologyModel, Relationship } from "./types";
import { EMPTY_MODEL, addEntityType, addRelationship, nextEntityColor } from "./state";
import { namespaceOf } from "./flowModel";

export type ExportFormat = "json" | "ttl" | "owl" | "jsonld" | "ntriples";

export const FORMAT_LABELS: Record<ExportFormat, string> = {
  json: "JSON",
  ttl: "Turtle (.ttl)",
  owl: "OWL/XML (.owl)",
  jsonld: "JSON-LD (.jsonld)",
  ntriples: "N-Triples (.nt)",
};

export const FORMAT_EXTENSIONS: Record<ExportFormat, string> = {
  json: "json",
  ttl: "ttl",
  owl: "owl",
  jsonld: "jsonld",
  ntriples: "nt",
};

const RDFS = "http://www.w3.org/2000/01/rdf-schema#";
const RDF = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const OWL = "http://www.w3.org/2002/07/owl#";

function localName(name: string): string {
  return name.replace(/\W+/g, "");
}

function classIri(base: string, name: string): string {
  return `${base}${encodeURIComponent(localName(name))}`;
}

function propIri(base: string, name: string): string {
  return `${base}${encodeURIComponent(localName(name).replace(/^[A-Z]/, (c) => c.toLowerCase()))}`;
}

function escapeTtlString(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n");
}

function escapeXml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function compactPropName(name: string): string {
  return localName(name).replace(/^[A-Z]/, (c) => c.toLowerCase());
}

export function exportTtl(model: OntologyModel, base: string): string {
  const lines: string[] = [
    `@prefix ex: <${base}> .`,
    "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .",
    "@prefix owl: <http://www.w3.org/2002/07/owl#> .",
    "",
  ];

  for (const entity of model.entityTypes) {
    const name = `ex:${localName(entity.name)}`;
    lines.push(`${name} a owl:Class ;`);
    lines.push(`  rdfs:label "${escapeTtlString(entity.displayName || entity.name)}" ;`);
    if (entity.description) {
      lines.push(`  rdfs:comment "${escapeTtlString(entity.description)}" ;`);
    }
    lines.push(`  rdfs:seeAlso "${escapeTtlString(entity.color)}" .`);
    lines.push("");
  }

  for (const rel of model.relationships) {
    const name = `ex:${compactPropName(rel.name)}`;
    const from = model.entityTypes.find((e) => e.id === rel.fromEntityTypeId);
    const to = model.entityTypes.find((e) => e.id === rel.toEntityTypeId);
    lines.push(`${name} a owl:ObjectProperty ;`);
    lines.push(`  rdfs:label "${escapeTtlString(rel.displayName || rel.name)}" ;`);
    if (rel.description) {
      lines.push(`  rdfs:comment "${escapeTtlString(rel.description)}" ;`);
    }
    if (from) {
      lines.push(`  rdfs:domain ex:${localName(from.name)} ;`);
    }
    if (to) {
      lines.push(`  rdfs:range ex:${localName(to.name)} ;`);
    }
    lines.push(`  rdfs:seeAlso "${escapeTtlString(rel.cardinality)}" .`);
    lines.push("");
  }

  return lines.join("\n");
}

export function exportJsonLd(model: OntologyModel, base: string): string {
  const graph = [
    ...model.entityTypes.map((entity) => ({
      "@id": classIri(base, entity.name),
      "@type": "owl:Class",
      label: entity.displayName || entity.name,
      ...(entity.description ? { comment: entity.description } : {}),
      seeAlso: entity.color,
    })),
    ...model.relationships.map((rel) => {
      const from = model.entityTypes.find((e) => e.id === rel.fromEntityTypeId);
      const to = model.entityTypes.find((e) => e.id === rel.toEntityTypeId);
      return {
        "@id": propIri(base, rel.name),
        "@type": "owl:ObjectProperty",
        label: rel.displayName || rel.name,
        ...(rel.description ? { comment: rel.description } : {}),
        ...(from ? { domain: classIri(base, from.name) } : {}),
        ...(to ? { range: classIri(base, to.name) } : {}),
        seeAlso: rel.cardinality,
      };
    }),
  ];

  const doc = {
    "@context": {
      rdfs: "http://www.w3.org/2000/01/rdf-schema#",
      owl: "http://www.w3.org/2002/07/owl#",
      label: "rdfs:label",
      comment: "rdfs:comment",
      domain: "rdfs:domain",
      range: "rdfs:range",
      seeAlso: "rdfs:seeAlso",
    },
    "@graph": graph,
  };

  return JSON.stringify(doc, null, 2);
}

export function exportNTriples(model: OntologyModel, base: string): string {
  const lines: string[] = [];

  for (const entity of model.entityTypes) {
    const iri = classIri(base, entity.name);
    lines.push(`<${iri}> <${RDF}type> <${OWL}Class> .`);
    lines.push(
      `<${iri}> <${RDFS}label> "${escapeTtlString(entity.displayName || entity.name)}" .`,
    );
    if (entity.description) {
      lines.push(`<${iri}> <${RDFS}comment> "${escapeTtlString(entity.description)}" .`);
    }
    lines.push(`<${iri}> <${RDFS}seeAlso> "${escapeTtlString(entity.color)}" .`);
  }

  for (const rel of model.relationships) {
    const iri = propIri(base, rel.name);
    const from = model.entityTypes.find((e) => e.id === rel.fromEntityTypeId);
    const to = model.entityTypes.find((e) => e.id === rel.toEntityTypeId);
    lines.push(`<${iri}> <${RDF}type> <${OWL}ObjectProperty> .`);
    lines.push(`<${iri}> <${RDFS}label> "${escapeTtlString(rel.displayName || rel.name)}" .`);
    if (rel.description) {
      lines.push(`<${iri}> <${RDFS}comment> "${escapeTtlString(rel.description)}" .`);
    }
    if (from) {
      lines.push(`<${iri}> <${RDFS}domain> <${classIri(base, from.name)}> .`);
    }
    if (to) {
      lines.push(`<${iri}> <${RDFS}range> <${classIri(base, to.name)}> .`);
    }
    lines.push(`<${iri}> <${RDFS}seeAlso> "${escapeTtlString(rel.cardinality)}" .`);
  }

  return lines.join("\n");
}

export function exportOwl(model: OntologyModel, base: string): string {
  const lines: string[] = [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"',
    '         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"',
    '         xmlns:owl="http://www.w3.org/2002/07/owl#">',
  ];

  for (const entity of model.entityTypes) {
    const iri = classIri(base, entity.name);
    lines.push(`  <owl:Class rdf:about="${escapeXml(iri)}">`);
    lines.push(
      `    <rdfs:label>${escapeXml(entity.displayName || entity.name)}</rdfs:label>`,
    );
    if (entity.description) {
      lines.push(`    <rdfs:comment>${escapeXml(entity.description)}</rdfs:comment>`);
    }
    lines.push(`    <rdfs:seeAlso>${escapeXml(entity.color)}</rdfs:seeAlso>`);
    lines.push("  </owl:Class>");
  }

  for (const rel of model.relationships) {
    const iri = propIri(base, rel.name);
    const from = model.entityTypes.find((e) => e.id === rel.fromEntityTypeId);
    const to = model.entityTypes.find((e) => e.id === rel.toEntityTypeId);
    lines.push(`  <owl:ObjectProperty rdf:about="${escapeXml(iri)}">`);
    lines.push(`    <rdfs:label>${escapeXml(rel.displayName || rel.name)}</rdfs:label>`);
    if (rel.description) {
      lines.push(`    <rdfs:comment>${escapeXml(rel.description)}</rdfs:comment>`);
    }
    if (from) {
      lines.push(
        `    <rdfs:domain rdf:resource="${escapeXml(classIri(base, from.name))}"/>`,
      );
    }
    if (to) {
      lines.push(
        `    <rdfs:range rdf:resource="${escapeXml(classIri(base, to.name))}"/>`,
      );
    }
    lines.push(`    <rdfs:seeAlso>${escapeXml(rel.cardinality)}</rdfs:seeAlso>`);
    lines.push("  </owl:ObjectProperty>");
  }

  lines.push("</rdf:RDF>");
  return lines.join("\n");
}

export function exportJson(model: OntologyModel): string {
  return JSON.stringify(model, null, 2);
}

export function exportModel(model: OntologyModel, format: ExportFormat, base: string): string {
  switch (format) {
    case "json":
      return exportJson(model);
    case "ttl":
      return exportTtl(model, base);
    case "owl":
      return exportOwl(model, base);
    case "jsonld":
      return exportJsonLd(model, base);
    case "ntriples":
      return exportNTriples(model, base);
    default:
      return exportJson(model);
  }
}

function expandIri(value: string, prefixes: Record<string, string>): string {
  if (value.startsWith("<") && value.endsWith(">")) {
    return value.slice(1, -1);
  }
  const colonIndex = value.indexOf(":");
  if (colonIndex > 0) {
    const prefix = value.slice(0, colonIndex);
    if (prefixes[prefix]) {
      return `${prefixes[prefix]}${value.slice(colonIndex + 1)}`;
    }
  }
  return value;
}

function parseLiteral(raw: string): string {
  const match = raw.match(/^"(.*)"(?:\^\^[^ ]+|@[a-z]+)?$/s);
  return match ? match[1]! : raw;
}

interface ParsedTriple {
  subject: string;
  predicate: string;
  object: string;
}

/** Strips a trailing `# comment`, but only where the `#` sits outside any
 *  `<...>` span. A bracket-blind `#[^\n]*` regex would also delete the rest
 *  of the line at the first `#` inside an IRI fragment (`.../rdf-schema#`,
 *  `...#Class`) — which is most real RDF/OWL IRIs. */
function stripLineComment(line: string): string {
  let insideIri = false;
  for (let i = 0; i < line.length; i += 1) {
    const ch = line[i];
    if (ch === "<") insideIri = true;
    else if (ch === ">") insideIri = false;
    else if (ch === "#" && !insideIri) return line.slice(0, i);
  }
  return line;
}

function stripComments(text: string): string {
  return text.split("\n").map(stripLineComment).join("\n");
}

function parseTtlTriples(text: string): ParsedTriple[] {
  const prefixes: Record<string, string> = {};
  const triples: ParsedTriple[] = [];

  const cleaned = stripComments(text)
    .replace(/\.[\s]*\./g, ".")
    .trim();

  // The lookahead requires actual whitespace (not just `\s*`) between the
  // terminating `.` and what follows — a statement boundary is always
  // `. <whitespace>`, while a domain-name dot ("example.org") is followed
  // immediately by a letter with no space. Matching on optional whitespace
  // here would split "example.org" into "example" and "org", corrupting
  // every IRI whose host contains a dot — which is nearly all of them.
  //
  // The lookahead's match is zero-width, so the whitespace it matched stays
  // on the front of the *next* statement — every statement past the first
  // therefore needs `.trim()` before an anchored `^` regex (the `@prefix`
  // check below) can match it.
  const statements = cleaned.split(/\s*\.(?=\s+[@<\w])/g).filter(Boolean);

  for (const rawStatement of statements) {
    const statement = rawStatement.trim();
    const prefixMatch = statement.match(/^@prefix\s+(\w+):\s*<([^>]+)>/);
    if (prefixMatch) {
      prefixes[prefixMatch[1]!] = prefixMatch[2]!;
      continue;
    }

    const parts = statement.trim().split(/\s+/);
    if (parts.length < 3) continue;

    const subject = expandIri(parts[0]!, prefixes);
    let index = 1;
    while (index < parts.length - 1) {
      // "a" is Turtle's own shorthand for rdf:type, not a prefixed name —
      // expandIri would otherwise pass it through unchanged (no colon to
      // expand), so every `ex:Customer a owl:Class` triple's predicate
      // would stay the literal string "a" and never match `${RDF}type`.
      const predicateToken = parts[index]!;
      const predicate = predicateToken === "a" ? `${RDF}type` : expandIri(predicateToken, prefixes);
      index += 1;
      if (index >= parts.length) break;
      let object = parts[index]!;
      index += 1;

      if (object === ";") continue;
      if (object === ",") {
        index -= 1;
        continue;
      }

      if (object.startsWith("\"")) {
        while (index <= parts.length && !object.endsWith("\"") && !object.match(/"\^\^|"@/)) {
          object += " " + parts[index]!;
          index += 1;
        }
        object = parseLiteral(object);
      } else {
        object = expandIri(object, prefixes);
      }

      triples.push({ subject, predicate, object });

      if (index < parts.length && parts[index] === ";") {
        index += 1;
      }
    }
  }

  return triples;
}

export function importTtl(text: string): OntologyModel {
  const triples = parseTtlTriples(text);
  return triplesToModel(triples);
}

function triplesToModel(triples: ParsedTriple[]): OntologyModel {
  let model = EMPTY_MODEL;
  const classMap = new Map<string, string>(); // iri -> entity id
  // `color` starts unset — most real ontologies (this builder's own
  // exportTtl round-trip aside) carry no colour hint at all, and defaulting
  // every class to the same fixed colour makes every node in the diagram
  // visually identical. A per-index palette colour is assigned below, only
  // for classes an explicit `rdfs:seeAlso` never overrides.
  const classes: Array<{ iri: string; label: string; comment: string; color: string | null }> = [];

  for (const { subject, predicate, object } of triples) {
    if (predicate === `${RDF}type` && (object === `${OWL}Class` || object.endsWith("#Class"))) {
      classes.push({ iri: subject, label: "", comment: "", color: null });
    }
  }

  const classByIri = new Map<string, (typeof classes)[number]>();
  for (const cls of classes) {
    classByIri.set(cls.iri, cls);
  }

  for (const { subject, predicate, object } of triples) {
    const cls = classByIri.get(subject);
    if (!cls) continue;
    if (predicate === `${RDFS}label`) cls.label = object;
    if (predicate === `${RDFS}comment`) cls.comment = object;
    if (predicate === `${RDFS}seeAlso`) cls.color = object;
  }

  classes.forEach((cls, index) => {
    model = addEntityType(model, {
      name: cls.label || cls.iri.split(/[#/]/).pop() || "Entity",
      displayName: cls.label || cls.iri.split(/[#/]/).pop() || "Entity",
      description: cls.comment,
      color: cls.color ?? nextEntityColor(index),
      namespace: namespaceOf(cls.iri),
    });
    classMap.set(cls.iri, model.entityTypes[model.entityTypes.length - 1]!.id);
  });

  interface RelAccumulator {
    displayName?: string;
    description?: string;
    cardinality?: Cardinality;
    from?: string;
    to?: string;
  }
  const relTriples = new Map<string, RelAccumulator>();
  for (const { subject, predicate, object } of triples) {
    const typeTriple = triples.find(
      (t) => t.subject === subject && t.predicate === `${RDF}type`,
    );
    if (
      typeTriple &&
      (typeTriple.object === `${OWL}ObjectProperty` || typeTriple.object.endsWith("#ObjectProperty"))
    ) {
      if (!relTriples.has(subject)) {
        relTriples.set(subject, {});
      }
      const rel = relTriples.get(subject)!;
      if (predicate === `${RDFS}label`) {
        rel.displayName = object;
      }
      if (predicate === `${RDFS}comment`) rel.description = object;
      if (predicate === `${RDFS}domain`) rel.from = object;
      if (predicate === `${RDFS}range`) rel.to = object;
      if (predicate === `${RDFS}seeAlso`) rel.cardinality = object as Relationship["cardinality"];
    }
  }

  for (const [subjectIri, rel] of relTriples) {
    const fromId = classMap.get(rel.from || "");
    const toId = classMap.get(rel.to || "");
    if (!fromId || !toId) continue;
    // The property's own technical name comes from its IRI's local part
    // (`ex:places` -> "places"), the same way a class's fallback name
    // does above — never from the label, which is display text and may
    // read differently ("Places" vs "places").
    const localPart = subjectIri.split(/[#/]/).pop() || "relatesTo";
    model = addRelationship(model, {
      fromEntityTypeId: fromId,
      toEntityTypeId: toId,
      name: localPart,
      displayName: rel.displayName || localPart,
      description: rel.description || "",
      cardinality: rel.cardinality || "oneToMany",
    });
  }

  return model;
}

export function importJsonLd(text: string): OntologyModel {
  const parsed = JSON.parse(text) as {
    "@context"?: Record<string, string>;
    "@graph"?: Array<Record<string, unknown>>;
  };
  const graph = parsed["@graph"] ?? [];
  let model = EMPTY_MODEL;
  const classMap = new Map<string, string>();
  // Counts classes only (not every graph node), so the palette rotates one
  // step per entity type regardless of how properties are interleaved.
  let classIndex = 0;

  for (const node of graph) {
    const type = node["@type"];
    const id = String(node["@id"] || "");
    const label = String(node.label || "");
    const comment = String(node.comment || "");
    const seeAlso = node.seeAlso ? String(node.seeAlso) : null;

    if (type === "owl:Class" || type === `${OWL}Class`) {
      model = addEntityType(model, {
        name: label || id.split(/[#/]/).pop() || "Entity",
        displayName: label || id.split(/[#/]/).pop() || "Entity",
        description: comment,
        color: seeAlso ?? nextEntityColor(classIndex),
        namespace: namespaceOf(id),
      });
      classIndex += 1;
      classMap.set(id, model.entityTypes[model.entityTypes.length - 1]!.id);
    }
  }

  for (const node of graph) {
    const type = node["@type"];
    if (type !== "owl:ObjectProperty" && type !== `${OWL}ObjectProperty`) continue;

    const id = String(node["@id"] || "");
    const label = String(node.label || "");
    const comment = String(node.comment || "");
    const from = String(node.domain || "");
    const to = String(node.range || "");
    const cardinality = String(node.seeAlso || "oneToMany");

    const fromId = classMap.get(from);
    const toId = classMap.get(to);
    if (!fromId || !toId) continue;

    model = addRelationship(model, {
      fromEntityTypeId: fromId,
      toEntityTypeId: toId,
      name: label || id.split(/[#/]/).pop() || "relatesTo",
      displayName: label || id.split(/[#/]/).pop() || "relatesTo",
      description: comment,
      cardinality: cardinality as Relationship["cardinality"],
    });
  }

  return model;
}

export function importNTriples(text: string): OntologyModel {
  const triples: ParsedTriple[] = text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"))
    .map((line) => {
      const match = line.match(/(<[^>]+>)\s+(<[^>]+>)\s+(.+)\s+\./);
      if (!match) return null;
      const [, subject, predicate, object] = match;
      return {
        subject: subject!.slice(1, -1),
        predicate: predicate!.slice(1, -1),
        object: object!.startsWith("<") ? object!.slice(1, -1) : parseLiteral(object!),
      };
    })
    .filter(Boolean) as ParsedTriple[];

  return triplesToModel(triples);
}

export function importModel(text: string, format: ExportFormat): OntologyModel {
  switch (format) {
    case "json":
      return JSON.parse(text) as OntologyModel;
    case "ttl":
      return importTtl(text);
    case "jsonld":
      return importJsonLd(text);
    case "ntriples":
      return importNTriples(text);
    case "owl":
      return importTtl(text);
    default:
      return JSON.parse(text) as OntologyModel;
  }
}
